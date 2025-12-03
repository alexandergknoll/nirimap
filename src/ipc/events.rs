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

/// Connect to the event stream
fn connect_event_stream() -> Result<BufReader<UnixStream>> {
    let socket_path = std::env::var("NIRI_SOCKET")
        .context("NIRI_SOCKET environment variable not set. Is Niri running?")?;

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

    // Extract position in scrolling layout (column, window_in_column)
    let (column_index, window_index) = layout
        .pos_in_scrolling_layout
        .map(|(c, w)| (c.saturating_sub(1), w.saturating_sub(1))) // Convert from 1-based to 0-based
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
    }
}
