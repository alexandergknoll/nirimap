use std::collections::HashMap;

/// Represents a single window in the minimap
#[derive(Debug, Clone)]
pub struct Window {
    /// Unique window identifier from Niri
    pub id: u64,
    /// Application identifier (e.g., "firefox", "alacritty")
    pub app_id: String,
    /// Window title
    pub title: String,
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
}

/// Represents a workspace containing windows
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// Workspace unique identifier
    pub id: u64,
    /// Workspace name (if set)
    pub name: Option<String>,
    /// Windows in this workspace, keyed by window ID
    pub windows: HashMap<u64, Window>,
    /// Whether this workspace is currently active
    pub is_active: bool,
}

impl Workspace {
    /// Calculate the total width of all columns in the workspace
    pub fn total_width(&self) -> f64 {
        if self.windows.is_empty() {
            return 0.0;
        }

        let mut max_x = 0.0f64;
        for window in self.windows.values() {
            let right_edge = window.pos.0 + window.size.0;
            max_x = max_x.max(right_edge);
        }
        max_x
    }

    /// Calculate the total height of the workspace
    pub fn total_height(&self) -> f64 {
        if self.windows.is_empty() {
            return 0.0;
        }

        let mut max_y = 0.0f64;
        for window in self.windows.values() {
            let bottom_edge = window.pos.1 + window.size.1;
            max_y = max_y.max(bottom_edge);
        }
        max_y
    }

    /// Get the minimum X position (leftmost window edge)
    pub fn min_x(&self) -> f64 {
        self.windows
            .values()
            .map(|w| w.pos.0)
            .fold(f64::INFINITY, f64::min)
    }
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
    /// Output/monitor name this minimap is displaying
    pub output_name: Option<String>,
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
                id: workspace_id,
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
        if let Some(workspace) = self.workspaces.get_mut(&workspace_id) {
            workspace.is_active = true;
        }
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.workspaces.clear();
        self.active_workspace_id = None;
        self.focused_window_id = None;
    }
}
