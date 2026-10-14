#![forbid(unsafe_code)]

mod config;
mod config_watcher;
mod ipc;
mod state;
mod ui;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use async_channel::Receiver;
use gtk4::glib;
use gtk4::prelude::*;

use config::Config;
use ipc::StateUpdate;
use ui::{create_layer_window, MinimapWidget};

const APP_ID: &str = "com.github.nirimap";

/// Pause after applying a batch of state updates. Updates arriving meanwhile
/// are drained and coalesced as one batch, bounding the relayouts per second
/// a client spamming changes can cause.
const UPDATE_BATCH_INTERVAL: Duration = Duration::from_millis(50);

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

    let (tx, rx) = async_channel::unbounded::<StateUpdate>();

    thread::spawn(move || {
        if let Err(e) = ipc::run_event_loop(move |update| {
            if tx.send_blocking(update).is_err() {
                tracing::warn!("Failed to send state update, receiver dropped");
            }
        }) {
            tracing::error!("IPC event loop error: {}", e);
        }
    });

    let (config_tx, config_rx) = async_channel::unbounded::<ConfigMessage>();

    let config_path = Config::config_path();
    thread::spawn(move || {
        let on_reload = move || config_tx.send_blocking(ConfigMessage::Reload).is_ok();
        if let Err(e) = config_watcher::watch_config_file(config_path, on_reload) {
            tracing::error!("Config watcher error: {}", e);
        }
    });

    let minimap_for_updates = minimap.clone();
    glib::spawn_future_local(apply_state_updates(rx, move |update| {
        apply_state_update(&minimap_for_updates, update);
    }));

    let minimap_for_config = minimap.clone();
    glib::spawn_future_local(async move {
        while let Ok(ConfigMessage::Reload) = config_rx.recv().await {
            minimap_for_config.reload_config();
        }
    });

    // present() is required for layer-shell.
    window.present();

    if !config.borrow().behavior.always_visible {
        minimap.hide();
    }

    tracing::info!("Nirimap window created and displayed");

    Ok(())
}

/// Pass IPC updates to `apply` on the main loop as they arrive, in batches.
/// Returns once the sender is dropped.
async fn apply_state_updates(rx: Receiver<StateUpdate>, mut apply: impl FnMut(StateUpdate)) {
    while let Ok(first) = rx.recv().await {
        for update in drain_state_updates(first, &rx) {
            apply(update);
        }
        glib::timeout_future(UPDATE_BATCH_INTERVAL).await;
    }
}

/// `first` plus everything already queued, with the backlog coalesced.
///
/// A client can change its title thousands of times per second, each arriving
/// as a `WindowChanged`. Taking the whole backlog per batch keeps the channel
/// bounded; [`coalesce_state_updates`] drops the redundant ones.
fn drain_state_updates(first: StateUpdate, rx: &Receiver<StateUpdate>) -> Vec<StateUpdate> {
    let mut updates = Vec::new();
    let queued = std::iter::from_fn(|| rx.try_recv().ok());
    for update in std::iter::once(first).chain(queued) {
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
            let change =
                minimap.update_state(|state| state.apply_window_change(window, workspace_id));

            // Only new windows on the active workspace trigger a show.
            if change.is_new && change.on_active_workspace {
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
                state.set_workspace_active_window(workspace_id, active_window_id);
            });
            tracing::debug!(
                "Workspace {} active window -> {:?}",
                workspace_id,
                active_window_id
            );
        }

        StateUpdate::LayoutsChanged(layouts) => {
            minimap.update_state(|state| {
                state.apply_layouts(&layouts);
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
        let (tx, rx) = async_channel::unbounded();
        tx.send_blocking(StateUpdate::WindowClosed(9)).unwrap();
        tx.send_blocking(StateUpdate::FullState(state::MinimapState::new()))
            .unwrap();
        tx.send_blocking(window_changed(2, "fresh")).unwrap();

        let kept = drain_state_updates(window_changed(1, "stale"), &rx);

        assert_eq!(kept.len(), 2);
        assert!(matches!(kept[0], StateUpdate::FullState(_)));
        assert_eq!(window_title(&kept[1]), Some("fresh"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_apply_state_updates_applies_first_at_once_then_batches() {
        use std::time::Instant;

        fn describe(update: &StateUpdate) -> String {
            match update {
                StateUpdate::WindowClosed(id) => format!("closed {id}"),
                other => window_title(other).unwrap_or("?").to_string(),
            }
        }

        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (tx, rx) = async_channel::unbounded();
                let applied = Rc::new(RefCell::new(Vec::new()));
                let applied_in_loop = applied.clone();
                let handle = glib::spawn_future_local(apply_state_updates(rx, move |update| {
                    applied_in_loop
                        .borrow_mut()
                        .push((describe(&update), Instant::now()));
                }));

                let iterate_until = |count: usize| {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while applied.borrow().len() < count {
                        assert!(Instant::now() < deadline, "timed out");
                        context.iteration(true);
                    }
                };

                tx.send_blocking(window_changed(1, "a")).unwrap();
                iterate_until(1);

                // A burst during the batch interval is applied as one coalesced batch.
                tx.send_blocking(window_changed(1, "b")).unwrap();
                tx.send_blocking(window_changed(1, "c")).unwrap();
                tx.send_blocking(StateUpdate::WindowClosed(5)).unwrap();
                iterate_until(3);

                drop(tx);
                context.block_on(handle).unwrap();

                let applied = applied.borrow();
                let names: Vec<&str> = applied.iter().map(|(name, _)| name.as_str()).collect();
                assert_eq!(names, ["a", "c", "closed 5"]);
                // Allow for timer granularity; glib timeouts never fire early.
                assert!(applied[1].1 - applied[0].1 >= Duration::from_millis(40));
            })
            .unwrap();
    }

    #[test]
    fn test_drain_with_empty_backlog_returns_first() {
        let (_tx, rx) = async_channel::unbounded();

        let kept = drain_state_updates(StateUpdate::WindowClosed(3), &rx);

        assert_eq!(kept.len(), 1);
        assert!(matches!(kept[0], StateUpdate::WindowClosed(3)));
    }

    #[test]
    fn test_drain_coalesces_first_with_backlog() {
        let (tx, rx) = async_channel::unbounded();
        tx.send_blocking(window_changed(1, "newer")).unwrap();

        let kept = drain_state_updates(window_changed(1, "older"), &rx);

        assert_eq!(kept.len(), 1);
        assert_eq!(window_title(&kept[0]), Some("newer"));
    }
}
