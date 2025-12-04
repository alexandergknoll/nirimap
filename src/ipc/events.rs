use anyhow::{Context, Result};
use niri_ipc::{Event, Request};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;

use crate::state::{MinimapState, Window, Workspace};

/// State update messages sent to the UI
#[derive(Debug, Clone)]
pub enum StateUpdate {
    /// Full state refresh
    FullState(MinimapState),
    /// A window was opened or changed
    WindowChanged(Window),
    /// A window was closed
    WindowClosed(u64),
    /// Window focus changed
    FocusChanged(Option<u64>),
    /// Active workspace changed
    WorkspaceActivated { id: u64, focused: bool },
    /// Window layouts changed
    LayoutsChanged(Vec<(u64, niri_ipc::WindowLayout)>),
}

/// Run the event loop, sending state updates to the provided sender
pub fn run_event_loop<F>(mut on_update: F) -> Result<()>
where
    F: FnMut(StateUpdate) + Send,
{
    // First, get initial state
    let initial_state = fetch_initial_state()?;
    on_update(StateUpdate::FullState(initial_state));

    // Then subscribe to event stream
    let reader = connect_event_stream()?;

    for line in reader.lines() {
        let line = line.context("Failed to read from event stream")?;

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        // Parse the event
        let event: Event = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse event: {}", line))?;

        // Convert to state update
        if let Some(update) = event_to_update(event) {
            on_update(update);
        }
    }

    Ok(())
}

/// Fetch the initial complete state from Niri
fn fetch_initial_state() -> Result<MinimapState> {
    let mut client = super::client::NiriClient::connect()?;

    let workspaces = client.get_workspaces()?;
    let windows = client.get_windows()?;

    let mut state = MinimapState::new();

    // Process workspaces
    for ws in workspaces {
        let workspace = Workspace {
            id: ws.id,
            name: ws.name,
            is_active: ws.is_focused, // is_focused means it's the globally focused workspace
            ..Default::default()
        };
        state.workspaces.insert(ws.id, workspace);

        if ws.is_focused {
            state.active_workspace_id = Some(ws.id);
        }
    }

    // Process windows
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

/// Validate the socket path for security
pub(super) fn validate_socket_path(socket_path: &str) -> Result<()> {
    use std::path::Path;

    let path = Path::new(socket_path);

    // Ensure the path is absolute (prevents relative path attacks)
    if !path.is_absolute() {
        anyhow::bail!("NIRI_SOCKET must be an absolute path, got: {}", socket_path);
    }

    // Check if the path is in expected locations for security
    // Typically: /run/user/<uid>/ or /tmp/
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

/// Connect to the event stream
fn connect_event_stream() -> Result<BufReader<UnixStream>> {
    let socket_path = std::env::var("NIRI_SOCKET")
        .context("NIRI_SOCKET environment variable not set. Is Niri running?")?;

    // Validate the socket path for security
    validate_socket_path(&socket_path)?;

    let stream = UnixStream::connect(&socket_path)
        .with_context(|| format!("Failed to connect to Niri socket at {}", socket_path))?;

    // Send the EventStream request
    let request = serde_json::to_string(&Request::EventStream)?;
    use std::io::Write;
    let mut writer = &stream;
    writeln!(writer, "{}", request)?;

    let mut reader = BufReader::new(stream);

    // Read and discard the initial reply ({"Ok":"Handled"})
    let mut reply_line = String::new();
    reader.read_line(&mut reply_line)
        .context("Failed to read EventStream reply")?;

    // Verify it was successful
    let reply: Result<niri_ipc::Response, String> = serde_json::from_str(&reply_line)
        .context("Failed to parse EventStream reply")?;

    if let Err(e) = reply {
        anyhow::bail!("EventStream request failed: {}", e);
    }

    tracing::debug!("Connected to event stream");

    Ok(reader)
}

/// Validate and convert 1-based indices from Niri to 0-based indices
/// Returns (column_index, window_index) as 0-based values
pub fn validate_and_convert_indices(col: usize, win_idx: usize, window_id: u64) -> (usize, usize) {
    // Validate indices are >= 1 (Niri uses 1-based indexing)
    if col == 0 {
        tracing::warn!("Invalid column index 0 received from Niri for window {}", window_id);
    }
    if win_idx == 0 {
        tracing::warn!("Invalid window index 0 received from Niri for window {}", window_id);
    }

    // Convert from 1-based to 0-based, saturating at 0 for invalid inputs
    (col.saturating_sub(1), win_idx.saturating_sub(1))
}

/// Convert a Niri event to a state update
fn event_to_update(event: Event) -> Option<StateUpdate> {
    match event {
        Event::WindowOpenedOrChanged { window } => {
            let model_window = niri_window_to_model(&window);
            Some(StateUpdate::WindowChanged(model_window))
        }
        Event::WindowClosed { id } => Some(StateUpdate::WindowClosed(id)),
        Event::WindowFocusChanged { id } => Some(StateUpdate::FocusChanged(id)),
        Event::WorkspaceActivated { id, focused } => {
            Some(StateUpdate::WorkspaceActivated { id, focused })
        }
        Event::WindowLayoutsChanged { changes } => {
            Some(StateUpdate::LayoutsChanged(changes))
        }
        // Ignore other events for now
        _ => None,
    }
}

/// Convert a niri-ipc Window to our model Window
fn niri_window_to_model(win: &niri_ipc::Window) -> Window {
    let layout = &win.layout;

    // Floating windows have pos_in_scrolling_layout = None
    let is_floating = layout.pos_in_scrolling_layout.is_none();

    // Extract position in scrolling layout (column, window_in_column)
    let (column_index, window_index) = layout
        .pos_in_scrolling_layout
        .map(|(c, w)| validate_and_convert_indices(c, w, win.id))
        .unwrap_or((0, 0));

    // Extract position in workspace view
    let pos = layout.tile_pos_in_workspace_view.unwrap_or((0.0, 0.0));

    Window {
        id: win.id,
        app_id: win.app_id.clone().unwrap_or_default(),
        title: win.title.clone().unwrap_or_default(),
        pos,
        size: layout.tile_size,
        column_index,
        window_index,
        is_focused: win.is_focused,
        is_floating,
    }
}
