use std::collections::HashMap;

/// Represents a single window in the minimap
#[derive(Debug, Clone)]
pub struct Window {
    /// Unique window identifier from Niri
    pub id: u64,
    /// Position in workspace view coordinates (x, y)
    pub pos: (f64, f64),
    /// Window tile size (width, height)
    pub size: (f64, f64),
    /// Column index in the scrolling layout
    pub column_index: usize,
    /// Window index within the column
    pub window_index: usize,
    /// Whether this window is currently focused
    pub is_focused: bool,
    /// Whether this window is floating (not tiled)
    pub is_floating: bool,
}

/// Represents a workspace containing windows
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// Windows in this workspace, keyed by window ID
    pub windows: HashMap<u64, Window>,
    /// Whether this workspace is currently active
    pub is_active: bool,
}


/// Main state container for the minimap
#[derive(Debug, Clone, Default)]
pub struct MinimapState {
    /// All workspaces, keyed by workspace ID
    pub workspaces: HashMap<u64, Workspace>,
    /// Currently active workspace ID
    pub active_workspace_id: Option<u64>,
    /// Currently focused window ID
    pub focused_window_id: Option<u64>,
}

impl MinimapState {
    /// Create a new empty state
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the currently active workspace, if any
    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id
            .and_then(|id| self.workspaces.get(&id))
    }

    /// Get a mutable reference to the active workspace
    pub fn active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.active_workspace_id
            .and_then(|id| self.workspaces.get_mut(&id))
    }

    /// Update or insert a window in the appropriate workspace
    pub fn upsert_window(&mut self, workspace_id: u64, window: Window) {
        let workspace = self.workspaces.entry(workspace_id).or_insert_with(|| {
            Workspace {
                ..Default::default()
            }
        });
        workspace.windows.insert(window.id, window);
    }

    /// Remove a window by ID from all workspaces
    pub fn remove_window(&mut self, window_id: u64) {
        for workspace in self.workspaces.values_mut() {
            workspace.windows.remove(&window_id);
        }
    }

    /// Set the focused window ID and update focus state
    pub fn set_focused_window(&mut self, window_id: Option<u64>) {
        // Clear old focus
        if let Some(old_id) = self.focused_window_id {
            for workspace in self.workspaces.values_mut() {
                if let Some(window) = workspace.windows.get_mut(&old_id) {
                    window.is_focused = false;
                }
            }
        }

        // Set new focus
        self.focused_window_id = window_id;
        if let Some(new_id) = window_id {
            for workspace in self.workspaces.values_mut() {
                if let Some(window) = workspace.windows.get_mut(&new_id) {
                    window.is_focused = true;
                }
            }
        }
    }

    /// Set the active workspace
    pub fn set_active_workspace(&mut self, workspace_id: u64) {
        // Clear old active state
        for workspace in self.workspaces.values_mut() {
            workspace.is_active = false;
        }

        // Set new active state
        self.active_workspace_id = Some(workspace_id);

        // Ensure the workspace exists (create if necessary for dynamically created workspaces)
        let workspace = self.workspaces.entry(workspace_id).or_insert_with(|| {
            Workspace {
                ..Default::default()
            }
        });
        workspace.is_active = true;
    }
}
