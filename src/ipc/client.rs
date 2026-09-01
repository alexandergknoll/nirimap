use anyhow::{Context, Result};
use niri_ipc::socket::Socket;
use niri_ipc::{Reply, Request, Response};

/// Request/response client for the Niri IPC socket.
pub struct NiriClient {
    socket: Socket,
}

impl NiriClient {
    pub fn connect() -> Result<Self> {
        let socket_path =
            std::env::var("NIRI_SOCKET").context("NIRI_SOCKET environment variable not set")?;
        super::events::validate_socket_path(&socket_path)?;

        let socket =
            Socket::connect().context("Failed to connect to Niri socket. Is Niri running?")?;
        Ok(Self { socket })
    }

    pub fn get_windows(&mut self) -> Result<Vec<niri_ipc::Window>> {
        let reply = self.send(Request::Windows)?;
        match reply {
            Response::Windows(windows) => Ok(windows),
            other => anyhow::bail!("Unexpected response for Windows request: {:?}", other),
        }
    }

    pub fn get_workspaces(&mut self) -> Result<Vec<niri_ipc::Workspace>> {
        let reply = self.send(Request::Workspaces)?;
        match reply {
            Response::Workspaces(workspaces) => Ok(workspaces),
            other => anyhow::bail!("Unexpected response for Workspaces request: {:?}", other),
        }
    }

    fn send(&mut self, request: Request) -> Result<Response> {
        let reply: Reply = self
            .socket
            .send(request)
            .context("Failed to send request to Niri")?;

        reply.map_err(|e| anyhow::anyhow!("Niri returned an error: {}", e))
    }
}
