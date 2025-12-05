mod config;
mod ipc;
mod state;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use gtk4::glib;
use gtk4::prelude::*;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use config::Config;
use ipc::StateUpdate;
use ui::{create_layer_window, MinimapWidget};

const APP_ID: &str = "com.github.nirimap";

/// Debounce duration for config reloads in milliseconds
/// Prevents excessive reloads when config file is modified multiple times rapidly
const CONFIG_RELOAD_DEBOUNCE_MS: u64 = 500;

/// Messages for config reload
enum ConfigMessage {
    Reload,
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting nirimap");

    // Load configuration
    let config = Config::load()?;
    tracing::info!("Loaded configuration from {:?}", Config::config_path());

    // Create GTK application
    let app = gtk4::Application::builder().application_id(APP_ID).build();

    // Wrap config in Rc<RefCell> for hot reload support
    let config = Rc::new(RefCell::new(config));
    let config_for_activate = config.clone();

    app.connect_activate(move |app| {
        if let Err(e) = activate(app, config_for_activate.clone()) {
            tracing::error!("Failed to activate application: {}", e);
        }
    });

    // Run the application
    let empty: Vec<String> = vec![];
    app.run_with_args(&empty);

    Ok(())
}

fn activate(app: &gtk4::Application, config: Rc<RefCell<Config>>) -> Result<()> {
    // Create the layer-shell window
    let window = create_layer_window(app, &config.borrow());

    // Create the minimap widget
    let minimap = MinimapWidget::new(config.clone());

    // Connect the window to the minimap for dynamic resizing
    minimap.set_window(window.clone());

    // Add the minimap widget to the window
    window.set_child(Some(minimap.widget()));

    // Set up channel for state updates from IPC thread
    let (tx, rx) = mpsc::channel::<StateUpdate>();

    // Start IPC event loop in a background thread
    thread::spawn(move || {
        if let Err(e) = ipc::run_event_loop(move |update| {
            if tx.send(update).is_err() {
                tracing::warn!("Failed to send state update, receiver dropped");
            }
        }) {
            tracing::error!("IPC event loop error: {}", e);
        }
    });

    // Set up channel for config reload messages
    let (config_tx, config_rx) = mpsc::channel::<ConfigMessage>();

    // Start file watcher in a background thread
    let config_path = Config::config_path();
    thread::spawn(move || {
        if let Err(e) = watch_config_file(config_path, config_tx) {
            tracing::error!("Config watcher error: {}", e);
        }
    });

    // Set up glib idle handler to process state updates and config reloads
    let minimap_clone = minimap.clone();
    let last_config_reload = Rc::new(RefCell::new(Instant::now()));
    let config_reload_debounce = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);

    glib::idle_add_local(move || {
        // Process all pending state updates
        while let Ok(update) = rx.try_recv() {
            apply_state_update(&minimap_clone, update);
        }

        // Process config reload messages with debouncing
        while let Ok(ConfigMessage::Reload) = config_rx.try_recv() {
            let now = Instant::now();
            let mut last_reload = last_config_reload.borrow_mut();

            // Only reload if enough time has passed since the last reload
            if now.duration_since(*last_reload) >= config_reload_debounce {
                minimap_clone.reload_config();
                *last_reload = now;
            } else {
                tracing::debug!("Config reload debounced (too soon after last reload)");
            }
        }

        glib::ControlFlow::Continue
    });

    // Show the window (present is required for layer-shell to work)
    window.present();

    // Hide immediately if not always visible
    if !config.borrow().behavior.always_visible {
        minimap.hide();
    }

    tracing::info!("Nirimap window created and displayed");

    Ok(())
}

/// Watch the config file for changes and send reload messages
fn watch_config_file(
    config_path: std::path::PathBuf,
    tx: mpsc::Sender<ConfigMessage>,
) -> Result<()> {
    let (watcher_tx, watcher_rx) = mpsc::channel::<Result<Event, notify::Error>>();

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = watcher_tx.send(res);
        },
        notify::Config::default(),
    )?;

    // Watch the config file's parent directory (to catch file replacements)
    if let Some(parent) = config_path.parent() {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
        tracing::info!("Watching config directory: {}", parent.display());
    }

    for event in watcher_rx {
        match event {
            Ok(event) => {
                // Check if the event is for our config file
                let is_config_event = event.paths.iter().any(|p| p == &config_path);

                if is_config_event {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            tracing::debug!("Config file changed, triggering reload");
                            if tx.send(ConfigMessage::Reload).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::warn!("File watcher error: {}", e);
            }
        }
    }

    Ok(())
}

/// Apply a state update to the minimap
fn apply_state_update(minimap: &MinimapWidget, update: StateUpdate) {
    match update {
        StateUpdate::FullState(new_state) => {
            minimap.update_state(|state| {
                *state = new_state;
            });
            tracing::debug!("Applied full state update");
        }

        StateUpdate::WindowChanged(window) => {
            let window_id = window.id;
            let is_focused = window.is_focused;
            let mut is_new_window = false;

            minimap.update_state(|state| {
                // If this window is focused, clear focus from all other windows first
                if is_focused {
                    state.set_focused_window(Some(window_id));
                }
                // Check if this is a new window or an update to existing
                if let Some(workspace) = state.active_workspace_mut() {
                    is_new_window = !workspace.windows.contains_key(&window_id);
                    workspace.windows.insert(window_id, window);
                }
            });

            // Only show the minimap for new windows, not property updates
            if is_new_window {
                minimap.show();
                tracing::debug!("New window {} opened (focused: {})", window_id, is_focused);
            } else {
                tracing::debug!("Window {} updated (focused: {})", window_id, is_focused);
            }
        }

        StateUpdate::WindowClosed(window_id) => {
            minimap.update_state(|state| {
                state.remove_window(window_id);
            });
            tracing::debug!("Window {} closed", window_id);
        }

        StateUpdate::FocusChanged(window_id) => {
            minimap.update_state(|state| {
                state.set_focused_window(window_id);
            });
            // Show the minimap only if focus changed to a different window
            minimap.show_on_focus_change(window_id);
            tracing::debug!("Focus changed to {:?}", window_id);
        }

        StateUpdate::WorkspaceActivated { id, focused } => {
            if focused {
                minimap.update_state(|state| {
                    state.set_active_workspace(id);
                });
                // Show the minimap when workspace changes (will auto-hide if configured)
                minimap.show();
                tracing::debug!("Workspace {} activated", id);
            }
        }

        StateUpdate::LayoutsChanged(layouts) => {
            minimap.update_state(|state| {
                for (window_id, layout) in layouts {
                    // Find and update the window's layout
                    for workspace in state.workspaces.values_mut() {
                        if let Some(window) = workspace.windows.get_mut(&window_id) {
                            window.pos = layout.tile_pos_in_workspace_view.unwrap_or(window.pos);
                            window.size = layout.tile_size;
                            // Update floating status
                            window.is_floating = layout.pos_in_scrolling_layout.is_none();
                            if let Some((col, win_idx)) = layout.pos_in_scrolling_layout {
                                let (column_index, window_index) =
                                    ipc::validate_and_convert_indices(col, win_idx, window_id);
                                window.column_index = column_index;
                                window.window_index = window_index;
                            }
                        }
                    }
                }
            });
            // Show the minimap when layouts change (window resize, move, etc.)
            minimap.show();
            tracing::debug!("Window layouts changed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_reload_debounce_constant() {
        // Verify the debounce constant is set to a reasonable value
        assert_eq!(CONFIG_RELOAD_DEBOUNCE_MS, 500);
    }

    #[test]
    fn test_debounce_logic_simulation() {
        // Simulate debouncing logic similar to what happens in activate()
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let mut last_reload = Instant::now();

        // Wait a bit less than the debounce duration
        std::thread::sleep(Duration::from_millis(100));
        let now = Instant::now();

        // Should be debounced (too soon)
        assert!(now.duration_since(last_reload) < debounce_duration);

        // Wait past the debounce duration
        std::thread::sleep(Duration::from_millis(450)); // Total: 550ms > 500ms
        let now = Instant::now();

        // Should not be debounced (enough time has passed)
        assert!(now.duration_since(last_reload) >= debounce_duration);

        // Update last_reload
        last_reload = now;

        // Immediate reload attempt should be debounced
        let now = Instant::now();
        assert!(now.duration_since(last_reload) < debounce_duration);
    }

    #[test]
    fn test_debounce_edge_case_exact_boundary() {
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let last_reload = Instant::now();

        // Sleep for exactly the debounce duration
        std::thread::sleep(debounce_duration);
        let now = Instant::now();

        // Should be >= debounce duration (edge case: exactly at boundary)
        assert!(now.duration_since(last_reload) >= debounce_duration);
    }

    #[test]
    fn test_debounce_multiple_rapid_events() {
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let mut last_reload = Instant::now();
        let mut reload_count = 0;

        // Simulate 10 rapid events over 200ms (all within debounce window)
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            let now = Instant::now();

            if now.duration_since(last_reload) >= debounce_duration {
                reload_count += 1;
                last_reload = now;
            }
        }

        // Only the first event should trigger a reload (200ms total < 500ms)
        assert_eq!(reload_count, 0);

        // Now wait long enough for the debounce to expire
        std::thread::sleep(Duration::from_millis(350)); // Total: 550ms > 500ms
        let now = Instant::now();

        if now.duration_since(last_reload) >= debounce_duration {
            reload_count += 1;
        }

        // Now we should get a reload
        assert_eq!(reload_count, 1);
    }
}
