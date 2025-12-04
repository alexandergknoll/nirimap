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
    /// Whether this window is floating (not tiled)
    pub is_floating: bool,
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

        // Ensure the workspace exists (create if necessary for dynamically created workspaces)
        let workspace = self.workspaces.entry(workspace_id).or_insert_with(|| {
            Workspace {
                id: workspace_id,
                ..Default::default()
            }
        });
        workspace.is_active = true;
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.workspaces.clear();
        self.active_workspace_id = None;
        self.focused_window_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_window(id: u64, x: f64, y: f64, width: f64, height: f64) -> Window {
        Window {
            id,
            app_id: format!("app_{}", id),
            title: format!("Window {}", id),
            pos: (x, y),
            size: (width, height),
            column_index: 0,
            window_index: 0,
            is_focused: false,
            is_floating: false,
        }
    }

    // Workspace tests
    #[test]
    fn test_workspace_total_width_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.total_width(), 0.0);
    }

    #[test]
    fn test_workspace_total_width_single_window() {
        let mut workspace = Workspace::default();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        workspace.windows.insert(1, window);

        assert_eq!(workspace.total_width(), 100.0);
    }

    #[test]
    fn test_workspace_total_width_multiple_windows() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 100.0, 0.0, 150.0, 200.0));
        workspace.windows.insert(3, create_test_window(3, 250.0, 0.0, 50.0, 200.0));

        // Right edge of window 3: 250 + 50 = 300
        assert_eq!(workspace.total_width(), 300.0);
    }

    #[test]
    fn test_workspace_total_height_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.total_height(), 0.0);
    }

    #[test]
    fn test_workspace_total_height_single_window() {
        let mut workspace = Workspace::default();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        workspace.windows.insert(1, window);

        assert_eq!(workspace.total_height(), 200.0);
    }

    #[test]
    fn test_workspace_total_height_stacked_windows() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 0.0, 0.0, 100.0, 100.0));
        workspace.windows.insert(2, create_test_window(2, 0.0, 100.0, 100.0, 150.0));
        workspace.windows.insert(3, create_test_window(3, 0.0, 250.0, 100.0, 75.0));

        // Bottom edge of window 3: 250 + 75 = 325
        assert_eq!(workspace.total_height(), 325.0);
    }

    #[test]
    fn test_workspace_min_x_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.min_x(), f64::INFINITY);
    }

    #[test]
    fn test_workspace_min_x_single_window() {
        let mut workspace = Workspace::default();
        let window = create_test_window(1, 50.0, 0.0, 100.0, 200.0);
        workspace.windows.insert(1, window);

        assert_eq!(workspace.min_x(), 50.0);
    }

    #[test]
    fn test_workspace_min_x_multiple_windows() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 100.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 50.0, 0.0, 150.0, 200.0));
        workspace.windows.insert(3, create_test_window(3, 200.0, 0.0, 50.0, 200.0));

        assert_eq!(workspace.min_x(), 50.0);
    }

    #[test]
    fn test_workspace_min_x_negative_coordinates() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, -50.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 0.0, 0.0, 150.0, 200.0));

        assert_eq!(workspace.min_x(), -50.0);
    }

    // MinimapState tests
    #[test]
    fn test_minimap_state_new() {
        let state = MinimapState::new();
        assert!(state.workspaces.is_empty());
        assert_eq!(state.active_workspace_id, None);
        assert_eq!(state.focused_window_id, None);
        assert_eq!(state.output_name, None);
    }

    #[test]
    fn test_minimap_state_active_workspace() {
        let mut state = MinimapState::new();

        // No active workspace initially
        assert!(state.active_workspace().is_none());

        // Add a workspace
        let workspace = Workspace {
            id: 1,
            is_active: true,
            ..Default::default()
        };
        state.workspaces.insert(1, workspace);
        state.active_workspace_id = Some(1);

        // Now we should get the workspace
        let active = state.active_workspace().unwrap();
        assert_eq!(active.id, 1);
        assert!(active.is_active);
    }

    #[test]
    fn test_minimap_state_upsert_window_new() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);

        state.upsert_window(1, window);

        // Workspace should be created
        assert!(state.workspaces.contains_key(&1));
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.id, 1);
        assert!(workspace.windows.contains_key(&1));
    }

    #[test]
    fn test_minimap_state_upsert_window_update() {
        let mut state = MinimapState::new();

        // Add initial window
        let window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window1);

        // Update the same window
        let mut window2 = create_test_window(1, 50.0, 50.0, 150.0, 250.0);
        window2.title = "Updated Window".to_string();
        state.upsert_window(1, window2);

        // Should still have only one window, but updated
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.windows.len(), 1);
        let window = workspace.windows.get(&1).unwrap();
        assert_eq!(window.title, "Updated Window");
        assert_eq!(window.pos, (50.0, 50.0));
        assert_eq!(window.size, (150.0, 250.0));
    }

    #[test]
    fn test_minimap_state_remove_window() {
        let mut state = MinimapState::new();

        // Add windows to multiple workspaces
        state.upsert_window(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        state.upsert_window(1, create_test_window(2, 100.0, 0.0, 100.0, 200.0));
        state.upsert_window(2, create_test_window(3, 0.0, 0.0, 100.0, 200.0));

        // Remove window 2
        state.remove_window(2);

        // Window 2 should be gone
        let workspace1 = state.workspaces.get(&1).unwrap();
        assert!(!workspace1.windows.contains_key(&2));
        assert!(workspace1.windows.contains_key(&1));

        // Other workspaces should be unaffected
        let workspace2 = state.workspaces.get(&2).unwrap();
        assert!(workspace2.windows.contains_key(&3));
    }

    #[test]
    fn test_minimap_state_set_focused_window() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        state.upsert_window(1, create_test_window(2, 100.0, 0.0, 100.0, 200.0));

        // Focus window 1
        state.set_focused_window(Some(1));
        assert_eq!(state.focused_window_id, Some(1));

        let workspace = state.workspaces.get(&1).unwrap();
        assert!(workspace.windows.get(&1).unwrap().is_focused);
        assert!(!workspace.windows.get(&2).unwrap().is_focused);

        // Switch focus to window 2
        state.set_focused_window(Some(2));
        assert_eq!(state.focused_window_id, Some(2));

        let workspace = state.workspaces.get(&1).unwrap();
        assert!(!workspace.windows.get(&1).unwrap().is_focused);
        assert!(workspace.windows.get(&2).unwrap().is_focused);
    }

    #[test]
    fn test_minimap_state_set_focused_window_none() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));

        // Focus window
        state.set_focused_window(Some(1));
        assert!(state.workspaces.get(&1).unwrap().windows.get(&1).unwrap().is_focused);

        // Clear focus
        state.set_focused_window(None);
        assert_eq!(state.focused_window_id, None);
        assert!(!state.workspaces.get(&1).unwrap().windows.get(&1).unwrap().is_focused);
    }

    #[test]
    fn test_minimap_state_set_active_workspace() {
        let mut state = MinimapState::new();
        state.workspaces.insert(1, Workspace { id: 1, ..Default::default() });
        state.workspaces.insert(2, Workspace { id: 2, ..Default::default() });

        // Set workspace 1 as active
        state.set_active_workspace(1);
        assert_eq!(state.active_workspace_id, Some(1));
        assert!(state.workspaces.get(&1).unwrap().is_active);
        assert!(!state.workspaces.get(&2).unwrap().is_active);

        // Switch to workspace 2
        state.set_active_workspace(2);
        assert_eq!(state.active_workspace_id, Some(2));
        assert!(!state.workspaces.get(&1).unwrap().is_active);
        assert!(state.workspaces.get(&2).unwrap().is_active);
    }

    #[test]
    fn test_minimap_state_set_active_workspace_creates_if_missing() {
        let mut state = MinimapState::new();

        // Set non-existent workspace as active
        state.set_active_workspace(99);

        // Workspace should be created
        assert!(state.workspaces.contains_key(&99));
        assert_eq!(state.active_workspace_id, Some(99));
        assert!(state.workspaces.get(&99).unwrap().is_active);
    }

    #[test]
    fn test_minimap_state_clear() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        state.set_active_workspace(1);
        state.set_focused_window(Some(1));
        state.output_name = Some("HDMI-1".to_string());

        state.clear();

        assert!(state.workspaces.is_empty());
        assert_eq!(state.active_workspace_id, None);
        assert_eq!(state.focused_window_id, None);
        // Note: clear() doesn't reset output_name, which is intentional
    }
}
