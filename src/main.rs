#![forbid(unsafe_code)]

mod config;
mod ipc;
mod state;
mod ui;

use std::cell::RefCell;
use std::collections::HashSet;
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

/// Minimum interval between config reloads.
const CONFIG_RELOAD_DEBOUNCE_MS: u64 = 500;

enum ConfigMessage {
    Reload,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting nirimap");

    let config = Config::load()?;
    tracing::info!("Loaded configuration from {:?}", Config::config_path());

    let app = gtk4::Application::builder().application_id(APP_ID).build();

    let config = Rc::new(RefCell::new(config));
    let config_for_activate = config.clone();

    app.connect_activate(move |app| {
        if let Err(e) = activate(app, config_for_activate.clone()) {
            tracing::error!("Failed to activate application: {}", e);
        }
    });

    let empty: Vec<String> = vec![];
    app.run_with_args(&empty);

    Ok(())
}

fn activate(app: &gtk4::Application, config: Rc<RefCell<Config>>) -> Result<()> {
    let window = create_layer_window(app, &config.borrow());
    let minimap = MinimapWidget::new(config.clone());
    minimap.set_window(window.clone());
    window.set_child(Some(minimap.widget()));

    let (tx, rx) = mpsc::channel::<StateUpdate>();

    thread::spawn(move || {
        if let Err(e) = ipc::run_event_loop(move |update| {
            if tx.send(update).is_err() {
                tracing::warn!("Failed to send state update, receiver dropped");
            }
        }) {
            tracing::error!("IPC event loop error: {}", e);
        }
    });

    let (config_tx, config_rx) = mpsc::channel::<ConfigMessage>();

    let config_path = Config::config_path();
    thread::spawn(move || {
        if let Err(e) = watch_config_file(config_path, config_tx) {
            tracing::error!("Config watcher error: {}", e);
        }
    });

    // Poll both channels from the GTK main loop.
    let minimap_clone = minimap.clone();
    let last_config_reload = Rc::new(RefCell::new(Instant::now()));
    let config_reload_debounce = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);

    glib::timeout_add_local(Duration::from_millis(50), move || {
        for update in drain_state_updates(&rx) {
            apply_state_update(&minimap_clone, update);
        }

        while let Ok(ConfigMessage::Reload) = config_rx.try_recv() {
            let now = Instant::now();
            let mut last_reload = last_config_reload.borrow_mut();

            if now.duration_since(*last_reload) >= config_reload_debounce {
                minimap_clone.reload_config();
                *last_reload = now;
            } else {
                tracing::debug!("Config reload debounced (too soon after last reload)");
            }
        }

        glib::ControlFlow::Continue
    });

    // present() is required for layer-shell.
    window.present();

    if !config.borrow().behavior.always_visible {
        minimap.hide();
    }

    tracing::info!("Nirimap window created and displayed");

    Ok(())
}

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

    // Watch the parent directory so atomic replaces (rename) are caught.
    if let Some(parent) = config_path.parent() {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
        tracing::info!("Watching config directory: {}", parent.display());
    }

    for event in watcher_rx {
        match event {
            Ok(event) => {
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

/// Drain the IPC channel and coalesce the backlog.
///
/// A client can change its title thousands of times per second, each arriving
/// as a `WindowChanged`. Taking the whole backlog per tick keeps the channel
/// bounded; [`coalesce_state_updates`] drops the redundant ones.
fn drain_state_updates(rx: &mpsc::Receiver<StateUpdate>) -> Vec<StateUpdate> {
    let mut updates = Vec::new();
    while let Ok(update) = rx.try_recv() {
        // A full snapshot supersedes everything queued before it.
        if matches!(update, StateUpdate::FullState(_)) {
            updates.clear();
        }
        updates.push(update);
    }
    coalesce_state_updates(updates)
}

/// Keep only the latest `WindowChanged` per window, at its original position so
/// ordering relative to other updates is preserved. Earlier ones are fully
/// superseded, so dropping them doesn't change the final state.
fn coalesce_state_updates(updates: Vec<StateUpdate>) -> Vec<StateUpdate> {
    let mut seen_windows = HashSet::new();
    let mut kept = Vec::with_capacity(updates.len());

    for update in updates.into_iter().rev() {
        let keep = match &update {
            StateUpdate::WindowChanged { window, .. } => seen_windows.insert(window.id),
            _ => true,
        };
        if keep {
            kept.push(update);
        }
    }

    kept.reverse();
    kept
}

fn apply_state_update(minimap: &MinimapWidget, update: StateUpdate) {
    match update {
        StateUpdate::FullState(new_state) => {
            minimap.update_state(|state| {
                *state = new_state;
            });
            tracing::debug!("Applied full state update");
        }

        StateUpdate::WindowChanged {
            window,
            workspace_id,
        } => {
            let window_id = window.id;
            let is_focused = window.is_focused;
            let is_floating = window.is_floating;
            let mut is_new_window = false;
            let mut is_on_active_workspace = false;

            minimap.update_state(|state| {
                if is_focused {
                    state.set_focused_window(Some(window_id));
                }

                if let Some(ws_id) = workspace_id {
                    is_on_active_workspace = state.active_workspace_id == Some(ws_id);

                    // Handles moves between workspaces.
                    for (&id, workspace) in state.workspaces.iter_mut() {
                        if id != ws_id {
                            workspace.windows.remove(&window_id);
                        }
                    }

                    let workspace = state
                        .workspaces
                        .entry(ws_id)
                        .or_insert_with(Default::default);
                    is_new_window = !workspace.windows.contains_key(&window_id);
                    workspace.windows.insert(window_id, window);
                }
            });

            // Only new windows on the active workspace trigger a show.
            if is_on_active_workspace && is_new_window {
                minimap.show_for_new_window(is_floating);
                tracing::debug!(
                    "New window {} opened (focused: {}, floating: {})",
                    window_id,
                    is_focused,
                    is_floating
                );
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
            minimap.show_on_focus_change(window_id);
            tracing::debug!("Focus changed to {:?}", window_id);
        }

        StateUpdate::WorkspaceActivated { id, focused } => {
            if focused {
                minimap.update_state(|state| {
                    state.set_active_workspace(id);
                });
                minimap.show();
                tracing::debug!("Workspace {} activated", id);
            }
        }

        StateUpdate::WorkspacesChanged(workspaces) => {
            minimap.update_state(|state| {
                state.replace_workspace_metadata(&workspaces);
            });
            tracing::debug!("Workspaces changed ({} total)", workspaces.len());
        }

        StateUpdate::WorkspaceActiveWindowChanged {
            workspace_id,
            active_window_id,
        } => {
            minimap.update_state(|state| {
                if let Some(ws) = state.workspaces.get_mut(&workspace_id) {
                    ws.active_window_id = active_window_id;
                }
            });
            tracing::debug!(
                "Workspace {} active window -> {:?}",
                workspace_id,
                active_window_id
            );
        }

        StateUpdate::LayoutsChanged(layouts) => {
            minimap.update_state(|state| {
                for (window_id, layout) in layouts {
                    for workspace in state.workspaces.values_mut() {
                        if let Some(window) = workspace.windows.get_mut(&window_id) {
                            window.pos = layout.tile_pos_in_workspace_view;
                            window.size = layout.tile_size;
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
            minimap.show();
            tracing::debug!("Window layouts changed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::Window;

    fn window_changed(id: u64, title: &str) -> StateUpdate {
        StateUpdate::WindowChanged {
            window: Window {
                id,
                pos: None,
                size: (100.0, 100.0),
                column_index: 0,
                window_index: 0,
                is_focused: false,
                is_floating: false,
                title: Some(title.to_string()),
                app_id: None,
            },
            workspace_id: Some(1),
        }
    }

    fn window_title(update: &StateUpdate) -> Option<&str> {
        match update {
            StateUpdate::WindowChanged { window, .. } => window.title.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn test_coalesce_keeps_only_latest_change_per_window() {
        // A client spamming title changes.
        let updates: Vec<StateUpdate> = (0..1000)
            .map(|i| window_changed(7, &format!("title {}", i)))
            .collect();

        let kept = coalesce_state_updates(updates);

        assert_eq!(kept.len(), 1);
        assert_eq!(window_title(&kept[0]), Some("title 999"));
    }

    #[test]
    fn test_coalesce_preserves_order_and_other_updates() {
        let updates = vec![
            window_changed(1, "a1"),
            StateUpdate::FocusChanged(Some(1)),
            window_changed(2, "b1"),
            window_changed(1, "a2"),
            StateUpdate::WindowClosed(2),
            StateUpdate::FocusChanged(Some(3)),
        ];

        let kept = coalesce_state_updates(updates);

        // Window 1's first change is dropped; everything else stays in order.
        assert_eq!(kept.len(), 5);
        assert!(matches!(kept[0], StateUpdate::FocusChanged(Some(1))));
        assert_eq!(window_title(&kept[1]), Some("b1"));
        assert_eq!(window_title(&kept[2]), Some("a2"));
        assert!(matches!(kept[3], StateUpdate::WindowClosed(2)));
        assert!(matches!(kept[4], StateUpdate::FocusChanged(Some(3))));
    }

    #[test]
    fn test_drain_full_state_supersedes_earlier_updates() {
        let (tx, rx) = mpsc::channel();
        tx.send(window_changed(1, "stale")).unwrap();
        tx.send(StateUpdate::WindowClosed(9)).unwrap();
        tx.send(StateUpdate::FullState(state::MinimapState::new()))
            .unwrap();
        tx.send(window_changed(2, "fresh")).unwrap();

        let kept = drain_state_updates(&rx);

        assert_eq!(kept.len(), 2);
        assert!(matches!(kept[0], StateUpdate::FullState(_)));
        assert_eq!(window_title(&kept[1]), Some("fresh"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_config_reload_debounce_constant() {
        assert_eq!(CONFIG_RELOAD_DEBOUNCE_MS, 500);
    }

    #[test]
    fn test_debounce_logic_simulation() {
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let mut last_reload = Instant::now();

        std::thread::sleep(Duration::from_millis(100));
        let now = Instant::now();

        assert!(now.duration_since(last_reload) < debounce_duration);

        std::thread::sleep(Duration::from_millis(450)); // total 550ms > 500ms
        let now = Instant::now();
        assert!(now.duration_since(last_reload) >= debounce_duration);

        last_reload = now;
        let now = Instant::now();
        assert!(now.duration_since(last_reload) < debounce_duration);
    }

    #[test]
    fn test_debounce_edge_case_exact_boundary() {
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let last_reload = Instant::now();

        std::thread::sleep(debounce_duration);
        let now = Instant::now();

        assert!(now.duration_since(last_reload) >= debounce_duration);
    }

    #[test]
    fn test_debounce_multiple_rapid_events() {
        let debounce_duration = Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS);
        let mut last_reload = Instant::now();
        let mut reload_count = 0;

        // 10 events over 200ms, all inside the debounce window
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            let now = Instant::now();

            if now.duration_since(last_reload) >= debounce_duration {
                reload_count += 1;
                last_reload = now;
            }
        }

        assert_eq!(reload_count, 0);

        std::thread::sleep(Duration::from_millis(350)); // total 550ms > 500ms
        let now = Instant::now();

        if now.duration_since(last_reload) >= debounce_duration {
            reload_count += 1;
        }

        assert_eq!(reload_count, 1);
    }
}
