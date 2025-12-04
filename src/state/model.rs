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

impl Workspace {
    /// Calculate the total width of the workspace (max x + width)
    pub fn total_width(&self) -> f64 {
        self.windows
            .values()
            .map(|w| w.pos.0 + w.size.0)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    }

    /// Calculate the total height of the workspace (max y + height)
    pub fn total_height(&self) -> f64 {
        self.windows
            .values()
            .map(|w| w.pos.1 + w.size.1)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    }

    /// Get the minimum x coordinate (leftmost edge)
    pub fn min_x(&self) -> f64 {
        self.windows
            .values()
            .map(|w| w.pos.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_window(id: u64, x: f64, y: f64, width: f64, height: f64) -> Window {
        Window {
            id,
            pos: (x, y),
            size: (width, height),
            column_index: 0,
            window_index: 0,
            is_focused: false,
            is_floating: false,
        }
    }

    #[test]
    fn test_workspace_total_width_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.total_width(), 0.0);
    }

    #[test]
    fn test_workspace_total_width_single_window() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        assert_eq!(workspace.total_width(), 100.0);
    }

    #[test]
    fn test_workspace_total_width_multiple_windows() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 150.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(3, create_test_window(3, 300.0, 0.0, 150.0, 200.0));
        // Max should be 300 + 150 = 450
        assert_eq!(workspace.total_width(), 450.0);
    }

    #[test]
    fn test_workspace_total_height_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.total_height(), 0.0);
    }

    #[test]
    fn test_workspace_total_height_stacked_windows() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, 0.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 0.0, 250.0, 100.0, 300.0));
        // Max should be 250 + 300 = 550
        assert_eq!(workspace.total_height(), 550.0);
    }

    #[test]
    fn test_workspace_min_x_empty() {
        let workspace = Workspace::default();
        assert_eq!(workspace.min_x(), 0.0);
    }

    #[test]
    fn test_workspace_min_x_with_negative() {
        let mut workspace = Workspace::default();
        workspace.windows.insert(1, create_test_window(1, -50.0, 0.0, 100.0, 200.0));
        workspace.windows.insert(2, create_test_window(2, 100.0, 0.0, 100.0, 200.0));
        assert_eq!(workspace.min_x(), -50.0);
    }

    #[test]
    fn test_minimap_state_new() {
        let state = MinimapState::new();
        assert!(state.workspaces.is_empty());
        assert_eq!(state.active_workspace_id, None);
        assert_eq!(state.focused_window_id, None);
    }

    #[test]
    fn test_minimap_state_upsert_window_new() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window);

        assert_eq!(state.workspaces.len(), 1);
        assert!(state.workspaces.get(&1).unwrap().windows.contains_key(&1));
    }

    #[test]
    fn test_minimap_state_upsert_window_update() {
        let mut state = MinimapState::new();
        let window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window1);

        // Update the same window
        let window2 = create_test_window(1, 50.0, 50.0, 150.0, 250.0);
        state.upsert_window(1, window2);

        assert_eq!(state.workspaces.len(), 1);
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.windows.len(), 1);
        let window = workspace.windows.get(&1).unwrap();
        assert_eq!(window.pos, (50.0, 50.0));
        assert_eq!(window.size, (150.0, 250.0));
    }

    #[test]
    fn test_minimap_state_remove_window() {
        let mut state = MinimapState::new();
        let window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        let window2 = create_test_window(2, 100.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window1);
        state.upsert_window(1, window2);

        assert_eq!(state.workspaces.get(&1).unwrap().windows.len(), 2);

        state.remove_window(1);
        assert_eq!(state.workspaces.get(&1).unwrap().windows.len(), 1);
        assert!(!state.workspaces.get(&1).unwrap().windows.contains_key(&1));
    }

    #[test]
    fn test_minimap_state_remove_window_across_workspaces() {
        let mut state = MinimapState::new();
        let window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window1.clone());
        state.upsert_window(2, window1);

        state.remove_window(1);

        // Window should be removed from both workspaces
        assert!(!state.workspaces.get(&1).unwrap().windows.contains_key(&1));
        assert!(!state.workspaces.get(&2).unwrap().windows.contains_key(&1));
    }

    #[test]
    fn test_minimap_state_set_focused_window() {
        let mut state = MinimapState::new();
        let mut window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        let mut window2 = create_test_window(2, 100.0, 0.0, 100.0, 200.0);
        window1.is_focused = true;
        state.upsert_window(1, window1);
        state.upsert_window(1, window2);
        state.focused_window_id = Some(1);

        // Change focus to window 2
        state.set_focused_window(Some(2));

        assert_eq!(state.focused_window_id, Some(2));
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.windows.get(&1).unwrap().is_focused, false);
        assert_eq!(workspace.windows.get(&2).unwrap().is_focused, true);
    }

    #[test]
    fn test_minimap_state_set_focused_window_none() {
        let mut state = MinimapState::new();
        let mut window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        window1.is_focused = true;
        state.upsert_window(1, window1);
        state.focused_window_id = Some(1);

        // Clear focus
        state.set_focused_window(None);

        assert_eq!(state.focused_window_id, None);
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.windows.get(&1).unwrap().is_focused, false);
    }

    #[test]
    fn test_minimap_state_set_active_workspace() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window.clone());
        state.upsert_window(2, window);
        state.workspaces.get_mut(&1).unwrap().is_active = true;
        state.active_workspace_id = Some(1);

        // Change active workspace
        state.set_active_workspace(2);

        assert_eq!(state.active_workspace_id, Some(2));
        assert_eq!(state.workspaces.get(&1).unwrap().is_active, false);
        assert_eq!(state.workspaces.get(&2).unwrap().is_active, true);
    }

    #[test]
    fn test_minimap_state_active_workspace() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window);
        state.set_active_workspace(1);

        let active = state.active_workspace();
        assert!(active.is_some());
        assert_eq!(active.unwrap().is_active, true);
    }

    #[test]
    fn test_minimap_state_active_workspace_none() {
        let state = MinimapState::new();
        assert!(state.active_workspace().is_none());
    }
}
