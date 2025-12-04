use anyhow::{Context, Result};
use niri_ipc::{Reply, Request, Response};
use niri_ipc::socket::Socket;

/// Client for communicating with Niri via IPC
pub struct NiriClient {
    socket: Socket,
}

impl NiriClient {
    /// Create a new client connected to the Niri socket
    pub fn connect() -> Result<Self> {
        // Validate socket path before connecting
        if let Ok(socket_path) = std::env::var("NIRI_SOCKET") {
            super::events::validate_socket_path(&socket_path)?;
        }

        let socket = Socket::connect().context("Failed to connect to Niri socket. Is Niri running?")?;
        Ok(Self { socket })
    }

    /// Query all windows
    pub fn get_windows(&mut self) -> Result<Vec<niri_ipc::Window>> {
        let reply = self.send(Request::Windows)?;
        match reply {
            Response::Windows(windows) => Ok(windows),
            other => anyhow::bail!("Unexpected response for Windows request: {:?}", other),
        }
    }

    /// Query all workspaces
    pub fn get_workspaces(&mut self) -> Result<Vec<niri_ipc::Workspace>> {
        let reply = self.send(Request::Workspaces)?;
        match reply {
            Response::Workspaces(workspaces) => Ok(workspaces),
            other => anyhow::bail!("Unexpected response for Workspaces request: {:?}", other),
        }
    }

    /// Query the focused window
    pub fn get_focused_window(&mut self) -> Result<Option<niri_ipc::Window>> {
        let reply = self.send(Request::FocusedWindow)?;
        match reply {
            Response::FocusedWindow(window) => Ok(window),
            other => anyhow::bail!("Unexpected response for FocusedWindow request: {:?}", other),
        }
    }

    /// Send a request and get a response
    fn send(&mut self, request: Request) -> Result<Response> {
        let reply: Reply = self
            .socket
            .send(request)
            .context("Failed to send request to Niri")?;

        reply.map_err(|e| anyhow::anyhow!("Niri returned an error: {}", e))
    }
}
