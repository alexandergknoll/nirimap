use anyhow::{Context, Result};
use niri_ipc::{Event, Request};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;

use crate::state::{MinimapState, Window, Workspace};

/// Updates sent from the IPC thread to the UI.
#[derive(Debug, Clone)]
pub enum StateUpdate {
    FullState(MinimapState),
    WindowChanged {
        window: Window,
        workspace_id: Option<u64>,
    },
    WindowClosed(u64),
    FocusChanged(Option<u64>),
    WorkspaceActivated {
        id: u64,
        focused: bool,
    },
    LayoutsChanged(Vec<(u64, niri_ipc::WindowLayout)>),
    /// Workspaces added, removed, or reordered
    WorkspacesChanged(Vec<niri_ipc::Workspace>),
    WorkspaceActiveWindowChanged {
        workspace_id: u64,
        active_window_id: Option<u64>,
    },
}

/// Fetch the initial state, then forward Niri events to `on_update` until the
/// stream ends.
pub fn run_event_loop<F>(mut on_update: F) -> Result<()>
where
    F: FnMut(StateUpdate) + Send,
{
    let initial_state = fetch_initial_state()?;
    on_update(StateUpdate::FullState(initial_state));

    let reader = connect_event_stream()?;

    for line in reader.lines() {
        let line = line.context("Failed to read from event stream")?;

        if line.trim().is_empty() {
            continue;
        }

        // Unknown events are skipped for forward compatibility.
        let event: Event = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(err) => {
                // Payloads may contain window titles; only log a bounded prefix at debug.
                tracing::warn!("Skipping unrecognized event: {}", err);
                tracing::debug!("Unrecognized event payload: {}", truncate_chars(&line, 200));
                continue;
            }
        };

        if let Some(update) = event_to_update(event) {
            on_update(update);
        }
    }

    Ok(())
}

fn fetch_initial_state() -> Result<MinimapState> {
    let mut client = super::client::NiriClient::connect()?;

    let workspaces = client.get_workspaces()?;
    let windows = client.get_windows()?;

    let mut state = MinimapState::new();

    for ws in workspaces {
        let workspace = Workspace {
            id: ws.id,
            idx: ws.idx,
            output: ws.output.clone(),
            is_active: ws.is_active,
            active_window_id: ws.active_window_id,
            ..Default::default()
        };
        state.workspaces.insert(ws.id, workspace);

        if ws.is_focused {
            state.active_workspace_id = Some(ws.id);
        }
    }

    for win in windows {
        if let Some(workspace_id) = win.workspace_id {
            let window = niri_window_to_model(&win);
            state.upsert_window(workspace_id, window);

            if win.is_focused {
                state.focused_window_id = Some(win.id);
            }
        }
    }

    Ok(state)
}

/// Truncate to `max_chars` characters without splitting a UTF-8 sequence.
fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

/// Reject relative socket paths; warn on unexpected locations.
pub(super) fn validate_socket_path(socket_path: &str) -> Result<()> {
    use std::path::Path;

    let path = Path::new(socket_path);

    if !path.is_absolute() {
        anyhow::bail!("NIRI_SOCKET must be an absolute path, got: {}", socket_path);
    }

    let path_str = socket_path;
    let is_expected_location = path_str.starts_with("/run/user/") || path_str.starts_with("/tmp/");

    if !is_expected_location {
        tracing::warn!(
            "NIRI_SOCKET is in an unexpected location: {}. Expected /run/user/<uid>/ or /tmp/",
            socket_path
        );
    }

    Ok(())
}

/// Open the Niri socket and subscribe to the event stream.
fn connect_event_stream() -> Result<BufReader<UnixStream>> {
    let socket_path = std::env::var("NIRI_SOCKET")
        .context("NIRI_SOCKET environment variable not set. Is Niri running?")?;

    validate_socket_path(&socket_path)?;

    let stream = UnixStream::connect(&socket_path)
        .with_context(|| format!("Failed to connect to Niri socket at {}", socket_path))?;

    let request = serde_json::to_string(&Request::EventStream)?;
    use std::io::Write;
    let mut writer = &stream;
    writeln!(writer, "{}", request)?;

    let mut reader = BufReader::new(stream);

    // The first line is the request reply ({"Ok":"Handled"}), not an event.
    let mut reply_line = String::new();
    reader
        .read_line(&mut reply_line)
        .context("Failed to read EventStream reply")?;

    let reply: Result<niri_ipc::Response, String> =
        serde_json::from_str(&reply_line).context("Failed to parse EventStream reply")?;

    if let Err(e) = reply {
        anyhow::bail!("EventStream request failed: {}", e);
    }

    tracing::debug!("Connected to event stream");

    Ok(reader)
}

/// Convert Niri's 1-based (column, window) indices to 0-based, warning on 0.
pub fn validate_and_convert_indices(col: usize, win_idx: usize, window_id: u64) -> (usize, usize) {
    if col == 0 {
        tracing::warn!(
            "Invalid column index 0 received from Niri for window {}",
            window_id
        );
    }
    if win_idx == 0 {
        tracing::warn!(
            "Invalid window index 0 received from Niri for window {}",
            window_id
        );
    }

    (col.saturating_sub(1), win_idx.saturating_sub(1))
}

fn event_to_update(event: Event) -> Option<StateUpdate> {
    match event {
        Event::WindowOpenedOrChanged { window } => {
            let workspace_id = window.workspace_id;
            let model_window = niri_window_to_model(&window);
            Some(StateUpdate::WindowChanged {
                window: model_window,
                workspace_id,
            })
        }
        Event::WindowClosed { id } => Some(StateUpdate::WindowClosed(id)),
        Event::WindowFocusChanged { id } => Some(StateUpdate::FocusChanged(id)),
        Event::WorkspaceActivated { id, focused } => {
            Some(StateUpdate::WorkspaceActivated { id, focused })
        }
        Event::WindowLayoutsChanged { changes } => Some(StateUpdate::LayoutsChanged(changes)),
        Event::WorkspacesChanged { workspaces } => Some(StateUpdate::WorkspacesChanged(workspaces)),
        Event::WorkspaceActiveWindowChanged {
            workspace_id,
            active_window_id,
        } => Some(StateUpdate::WorkspaceActiveWindowChanged {
            workspace_id,
            active_window_id,
        }),
        _ => None,
    }
}

fn niri_window_to_model(win: &niri_ipc::Window) -> Window {
    let layout = &win.layout;

    // Floating windows have no scrolling-layout position.
    let is_floating = layout.pos_in_scrolling_layout.is_none();

    let (column_index, window_index) = layout
        .pos_in_scrolling_layout
        .map(|(c, w)| validate_and_convert_indices(c, w, win.id))
        .unwrap_or((0, 0));

    Window {
        id: win.id,
        pos: layout.tile_pos_in_workspace_view,
        size: layout.tile_size,
        column_index,
        window_index,
        is_focused: win.is_focused,
        is_floating,
        title: win.title.clone(),
        app_id: win.app_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_short_string_unchanged() {
        assert_eq!(truncate_chars("hello", 100), "hello");
        assert_eq!(truncate_chars("", 100), "");
        assert_eq!(truncate_chars("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_chars_ascii() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_chars_does_not_split_multibyte() {
        // Each emoji is 4 bytes; a byte-index slice at 5 would panic.
        let s = "🦀🦀🦀";
        assert_eq!(truncate_chars(s, 1), "🦀");
        assert_eq!(truncate_chars(s, 2), "🦀🦀");
        assert_eq!(truncate_chars(s, 3), s);

        let mixed = "ab日本語cd";
        assert_eq!(truncate_chars(mixed, 3), "ab日");
        assert_eq!(truncate_chars(mixed, 0), "");
    }

    #[test]
    fn test_validate_socket_path_valid_absolute_paths() {
        assert!(validate_socket_path("/run/user/1000/niri.sock").is_ok());
        assert!(validate_socket_path("/tmp/niri.sock").is_ok());
        assert!(validate_socket_path("/run/user/12345/niri-wayland.sock").is_ok());
    }

    #[test]
    fn test_validate_socket_path_rejects_relative_paths() {
        assert!(validate_socket_path("./niri.sock").is_err());
        assert!(validate_socket_path("../niri.sock").is_err());
        assert!(validate_socket_path("niri.sock").is_err());
    }

    #[test]
    fn test_validate_socket_path_empty() {
        assert!(validate_socket_path("").is_err());
    }

    #[test]
    fn test_validate_socket_path_unexpected_location() {
        // Unexpected locations only warn.
        assert!(validate_socket_path("/home/user/niri.sock").is_ok());
        assert!(validate_socket_path("/var/niri.sock").is_ok());
    }

    #[test]
    fn test_validate_and_convert_indices_normal_conversion() {
        assert_eq!(validate_and_convert_indices(1, 1, 100), (0, 0));
        assert_eq!(validate_and_convert_indices(2, 1, 100), (1, 0));
        assert_eq!(validate_and_convert_indices(1, 2, 100), (0, 1));
        assert_eq!(validate_and_convert_indices(5, 3, 100), (4, 2));
    }

    #[test]
    fn test_validate_and_convert_indices_zero_handling() {
        assert_eq!(validate_and_convert_indices(0, 1, 100), (0, 0));
        assert_eq!(validate_and_convert_indices(1, 0, 100), (0, 0));
        assert_eq!(validate_and_convert_indices(0, 0, 100), (0, 0));
    }

    #[test]
    fn test_validate_and_convert_indices_max_value() {
        assert_eq!(
            validate_and_convert_indices(usize::MAX, 1, 100),
            (usize::MAX - 1, 0)
        );
        assert_eq!(
            validate_and_convert_indices(1, usize::MAX, 100),
            (0, usize::MAX - 1)
        );
        assert_eq!(
            validate_and_convert_indices(usize::MAX, usize::MAX, 100),
            (usize::MAX - 1, usize::MAX - 1)
        );
    }

    #[test]
    fn test_validate_and_convert_indices_large_values() {
        assert_eq!(validate_and_convert_indices(1000, 500, 100), (999, 499));
        assert_eq!(
            validate_and_convert_indices(999999, 123456, 100),
            (999998, 123455)
        );
    }
}
