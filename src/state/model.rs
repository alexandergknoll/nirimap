use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Window {
    pub id: u64,
    /// Position in workspace-view coordinates. Niri only reports this for tiles
    /// currently in the viewport, so `None` must not be treated as `(0, 0)`.
    pub pos: Option<(f64, f64)>,
    /// Tile size (width, height)
    pub size: (f64, f64),
    /// Column index in the scrolling layout
    pub column_index: usize,
    /// Window index within the column
    pub window_index: usize,
    pub is_focused: bool,
    pub is_floating: bool,
    pub title: Option<String>,
    pub app_id: Option<String>,
}

impl Window {
    pub fn from_niri(win: &niri_ipc::Window) -> Self {
        let mut window = Self {
            id: win.id,
            pos: None,
            size: (0.0, 0.0),
            column_index: 0,
            window_index: 0,
            is_focused: win.is_focused,
            is_floating: false,
            title: win.title.clone(),
            app_id: win.app_id.clone(),
        };
        window.apply_layout(&win.layout);
        window
    }

    /// Update the geometry from a Niri layout. Floating windows have no
    /// scrolling-layout position, so their column/window indices are kept.
    pub fn apply_layout(&mut self, layout: &niri_ipc::WindowLayout) {
        self.pos = layout.tile_pos_in_workspace_view;
        self.size = layout.tile_size;
        self.is_floating = layout.pos_in_scrolling_layout.is_none();
        if let Some((col, win_idx)) = layout.pos_in_scrolling_layout {
            (self.column_index, self.window_index) = to_zero_based_indices(col, win_idx, self.id);
        }
    }
}

/// Convert Niri's 1-based (column, window) indices to 0-based, warning on 0.
fn to_zero_based_indices(col: usize, win_idx: usize, window_id: u64) -> (usize, usize) {
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

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    /// Niri workspace id (stable across moves/reorder)
    pub id: u64,
    /// Index on its output (display order)
    pub idx: u8,
    pub output: Option<String>,
    pub windows: HashMap<u64, Window>,
    pub is_active: bool,
    /// Most-recently-focused window on this workspace; Niri uses it for
    /// Overview-style alignment.
    pub active_window_id: Option<u64>,
}

/// Outcome of [`MinimapState::apply_window_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowChange {
    /// The window wasn't on its workspace before: it just opened or moved there.
    pub is_new: bool,
    /// The window's workspace is the active one.
    pub on_active_workspace: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MinimapState {
    pub workspaces: HashMap<u64, Workspace>,
    pub active_workspace_id: Option<u64>,
    pub focused_window_id: Option<u64>,
}

impl MinimapState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id
            .and_then(|id| self.workspaces.get(&id))
    }

    /// Find a window by id across all workspaces.
    pub fn find_window(&self, id: u64) -> Option<&Window> {
        self.workspaces.values().find_map(|ws| ws.windows.get(&id))
    }

    /// Workspaces sorted by output (`None` last), then `idx`.
    pub fn workspaces_sorted(&self) -> Vec<&Workspace> {
        let mut out: Vec<&Workspace> = self.workspaces.values().collect();
        out.sort_by(|a, b| {
            a.output
                .is_none()
                .cmp(&b.output.is_none())
                .then_with(|| a.output.cmp(&b.output))
                .then_with(|| a.idx.cmp(&b.idx))
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    /// Sync workspace metadata from a fresh snapshot, keeping the windows of
    /// workspaces that still exist. Missing workspaces are dropped and new ones
    /// inserted empty. `active_workspace_id` follows the focused workspace,
    /// falling back to the previous value if that workspace still exists.
    pub fn replace_workspace_metadata(&mut self, incoming: &[niri_ipc::Workspace]) {
        use std::collections::HashSet;

        let incoming_ids: HashSet<u64> = incoming.iter().map(|w| w.id).collect();
        self.workspaces.retain(|id, _| incoming_ids.contains(id));

        for ws in incoming {
            let entry = self.workspaces.entry(ws.id).or_insert_with(|| Workspace {
                id: ws.id,
                ..Default::default()
            });
            entry.id = ws.id;
            entry.idx = ws.idx;
            entry.output = ws.output.clone();
            entry.is_active = ws.is_active;
            entry.active_window_id = ws.active_window_id;
        }

        self.active_workspace_id = incoming.iter().find(|w| w.is_focused).map(|w| w.id).or(self
            .active_workspace_id
            .filter(|id| incoming_ids.contains(id)));
    }

    /// The workspace with `workspace_id`, inserted empty if it isn't known yet.
    fn workspace_entry(&mut self, workspace_id: u64) -> &mut Workspace {
        self.workspaces
            .entry(workspace_id)
            .or_insert_with(|| Workspace {
                id: workspace_id,
                ..Default::default()
            })
    }

    pub fn upsert_window(&mut self, workspace_id: u64, window: Window) {
        self.workspace_entry(workspace_id)
            .windows
            .insert(window.id, window);
    }

    /// Apply a window opened/changed event. The window is moved to
    /// `workspace_id` if it was on another workspace. Windows without a
    /// workspace aren't stored, but still take focus if they report it.
    pub fn apply_window_change(
        &mut self,
        window: Window,
        workspace_id: Option<u64>,
    ) -> WindowChange {
        if window.is_focused {
            self.set_focused_window(Some(window.id));
        }

        let Some(workspace_id) = workspace_id else {
            return WindowChange {
                is_new: false,
                on_active_workspace: false,
            };
        };

        for (&id, workspace) in self.workspaces.iter_mut() {
            if id != workspace_id {
                workspace.windows.remove(&window.id);
            }
        }

        let on_active_workspace = self.active_workspace_id == Some(workspace_id);
        let is_new = self
            .workspace_entry(workspace_id)
            .windows
            .insert(window.id, window)
            .is_none();

        WindowChange {
            is_new,
            on_active_workspace,
        }
    }

    /// Patch the geometry of known windows; unknown window ids are ignored.
    pub fn apply_layouts(&mut self, changes: &[(u64, niri_ipc::WindowLayout)]) {
        for (window_id, layout) in changes {
            for workspace in self.workspaces.values_mut() {
                if let Some(window) = workspace.windows.get_mut(window_id) {
                    window.apply_layout(layout);
                }
            }
        }
    }

    /// Record the most-recently-focused window of a known workspace.
    pub fn set_workspace_active_window(&mut self, workspace_id: u64, window_id: Option<u64>) {
        if let Some(workspace) = self.workspaces.get_mut(&workspace_id) {
            workspace.active_window_id = window_id;
        }
    }

    /// Remove a window from every workspace.
    pub fn remove_window(&mut self, window_id: u64) {
        for workspace in self.workspaces.values_mut() {
            workspace.windows.remove(&window_id);
        }
    }

    /// Move focus to `window_id`, updating `is_focused` flags.
    pub fn set_focused_window(&mut self, window_id: Option<u64>) {
        if let Some(old_id) = self.focused_window_id {
            for workspace in self.workspaces.values_mut() {
                if let Some(window) = workspace.windows.get_mut(&old_id) {
                    window.is_focused = false;
                }
            }
        }

        self.focused_window_id = window_id;
        if let Some(new_id) = window_id {
            for workspace in self.workspaces.values_mut() {
                if let Some(window) = workspace.windows.get_mut(&new_id) {
                    window.is_focused = true;
                }
            }
        }
    }

    pub fn set_active_workspace(&mut self, workspace_id: u64) {
        for workspace in self.workspaces.values_mut() {
            workspace.is_active = false;
        }

        self.active_workspace_id = Some(workspace_id);

        // The workspace may not be known yet (dynamically created).
        self.workspace_entry(workspace_id).is_active = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_window(id: u64, x: f64, y: f64, width: f64, height: f64) -> Window {
        Window {
            id,
            pos: Some((x, y)),
            size: (width, height),
            column_index: 0,
            window_index: 0,
            is_focused: false,
            is_floating: false,
            title: None,
            app_id: None,
        }
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

        let window2 = create_test_window(1, 50.0, 50.0, 150.0, 250.0);
        state.upsert_window(1, window2);

        assert_eq!(state.workspaces.len(), 1);
        let workspace = state.workspaces.get(&1).unwrap();
        assert_eq!(workspace.windows.len(), 1);
        let window = workspace.windows.get(&1).unwrap();
        assert_eq!(window.pos, Some((50.0, 50.0)));
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

        assert!(!state.workspaces.get(&1).unwrap().windows.contains_key(&1));
        assert!(!state.workspaces.get(&2).unwrap().windows.contains_key(&1));
    }

    #[test]
    fn test_minimap_state_set_focused_window() {
        let mut state = MinimapState::new();
        let mut window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        let window2 = create_test_window(2, 100.0, 0.0, 100.0, 200.0);
        window1.is_focused = true;
        state.upsert_window(1, window1);
        state.upsert_window(1, window2);
        state.focused_window_id = Some(1);

        state.set_focused_window(Some(2));

        assert_eq!(state.focused_window_id, Some(2));
        let workspace = state.workspaces.get(&1).unwrap();
        assert!(!workspace.windows.get(&1).unwrap().is_focused);
        assert!(workspace.windows.get(&2).unwrap().is_focused);
    }

    #[test]
    fn test_minimap_state_set_focused_window_none() {
        let mut state = MinimapState::new();
        let mut window1 = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        window1.is_focused = true;
        state.upsert_window(1, window1);
        state.focused_window_id = Some(1);

        state.set_focused_window(None);

        assert_eq!(state.focused_window_id, None);
        let workspace = state.workspaces.get(&1).unwrap();
        assert!(!workspace.windows.get(&1).unwrap().is_focused);
    }

    #[test]
    fn test_minimap_state_set_active_workspace() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window.clone());
        state.upsert_window(2, window);
        state.workspaces.get_mut(&1).unwrap().is_active = true;
        state.active_workspace_id = Some(1);

        state.set_active_workspace(2);

        assert_eq!(state.active_workspace_id, Some(2));
        assert!(!state.workspaces.get(&1).unwrap().is_active);
        assert!(state.workspaces.get(&2).unwrap().is_active);
    }

    #[test]
    fn test_minimap_state_active_workspace() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window);
        state.set_active_workspace(1);

        let active = state.active_workspace();
        assert!(active.is_some());
        assert!(active.unwrap().is_active);
    }

    #[test]
    fn test_minimap_state_active_workspace_none() {
        let state = MinimapState::new();
        assert!(state.active_workspace().is_none());
    }

    fn ipc_workspace(
        id: u64,
        idx: u8,
        output: Option<&str>,
        is_active: bool,
        is_focused: bool,
    ) -> niri_ipc::Workspace {
        niri_ipc::Workspace {
            id,
            idx,
            name: None,
            output: output.map(|s| s.to_string()),
            is_urgent: false,
            is_active,
            is_focused,
            active_window_id: None,
        }
    }

    #[test]
    fn test_workspaces_sorted_by_output_then_idx() {
        let mut state = MinimapState::new();
        let incoming = vec![
            ipc_workspace(3, 2, Some("DP-1"), false, false),
            ipc_workspace(1, 1, Some("DP-1"), true, true),
            ipc_workspace(5, 1, Some("HDMI-1"), true, false),
            ipc_workspace(7, 99, None, false, false),
            ipc_workspace(4, 3, Some("DP-1"), false, false),
        ];
        state.replace_workspace_metadata(&incoming);

        let sorted_ids: Vec<u64> = state.workspaces_sorted().iter().map(|w| w.id).collect();
        // DP-1: idx 1, 2, 3 -> 1, 3, 4; then HDMI-1 idx 1 -> 5; then no-output -> 7
        assert_eq!(sorted_ids, vec![1, 3, 4, 5, 7]);
    }

    #[test]
    fn test_replace_workspace_metadata_preserves_windows() {
        let mut state = MinimapState::new();
        let window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        state.upsert_window(1, window);

        assert_eq!(state.workspaces.get(&1).unwrap().windows.len(), 1);

        let incoming = vec![ipc_workspace(1, 0, Some("DP-1"), true, true)];
        state.replace_workspace_metadata(&incoming);

        let ws = state.workspaces.get(&1).unwrap();
        assert_eq!(ws.idx, 0);
        assert_eq!(ws.output.as_deref(), Some("DP-1"));
        assert!(ws.is_active);
        assert_eq!(ws.windows.len(), 1);
        assert_eq!(state.active_workspace_id, Some(1));
    }

    #[test]
    fn test_replace_workspace_metadata_removes_missing() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(10, 0.0, 0.0, 100.0, 200.0));
        state.upsert_window(2, create_test_window(20, 0.0, 0.0, 100.0, 200.0));
        state.active_workspace_id = Some(2);

        let incoming = vec![ipc_workspace(1, 0, None, true, true)];
        state.replace_workspace_metadata(&incoming);

        assert!(state.workspaces.contains_key(&1));
        assert!(!state.workspaces.contains_key(&2));
        assert_eq!(state.active_workspace_id, Some(1));
    }

    #[test]
    fn test_replace_workspace_metadata_inserts_new_empty() {
        let mut state = MinimapState::new();
        let incoming = vec![
            ipc_workspace(1, 0, None, true, true),
            ipc_workspace(2, 1, None, false, false),
        ];
        state.replace_workspace_metadata(&incoming);

        assert_eq!(state.workspaces.len(), 2);
        assert!(state.workspaces.get(&1).unwrap().windows.is_empty());
        assert!(state.workspaces.get(&2).unwrap().windows.is_empty());
    }

    fn ipc_layout(
        pos_in_scrolling_layout: Option<(usize, usize)>,
        tile_pos: Option<(f64, f64)>,
        tile_size: (f64, f64),
    ) -> niri_ipc::WindowLayout {
        niri_ipc::WindowLayout {
            pos_in_scrolling_layout,
            tile_size,
            window_size: (tile_size.0 as i32, tile_size.1 as i32),
            tile_pos_in_workspace_view: tile_pos,
            window_offset_in_tile: (0.0, 0.0),
        }
    }

    fn ipc_window(id: u64, is_focused: bool, layout: niri_ipc::WindowLayout) -> niri_ipc::Window {
        niri_ipc::Window {
            id,
            title: Some("title".to_string()),
            app_id: Some("app".to_string()),
            pid: None,
            workspace_id: Some(1),
            is_focused,
            is_floating: layout.pos_in_scrolling_layout.is_none(),
            is_urgent: false,
            layout,
            focus_timestamp: None,
        }
    }

    #[test]
    fn test_window_from_niri_tiled() {
        let layout = ipc_layout(Some((3, 2)), Some((10.0, 20.0)), (800.0, 600.0));
        let window = Window::from_niri(&ipc_window(7, true, layout));

        assert_eq!(window.id, 7);
        assert_eq!(window.pos, Some((10.0, 20.0)));
        assert_eq!(window.size, (800.0, 600.0));
        assert_eq!((window.column_index, window.window_index), (2, 1));
        assert!(window.is_focused);
        assert!(!window.is_floating);
        assert_eq!(window.title.as_deref(), Some("title"));
        assert_eq!(window.app_id.as_deref(), Some("app"));
    }

    #[test]
    fn test_window_from_niri_floating() {
        let layout = ipc_layout(None, None, (300.0, 200.0));
        let window = Window::from_niri(&ipc_window(7, false, layout));

        assert!(window.is_floating);
        assert_eq!(window.pos, None);
        assert_eq!(window.size, (300.0, 200.0));
        assert_eq!((window.column_index, window.window_index), (0, 0));
    }

    #[test]
    fn test_apply_layout_updates_geometry() {
        let mut window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        window.apply_layout(&ipc_layout(Some((2, 3)), None, (400.0, 500.0)));

        assert_eq!(window.pos, None);
        assert_eq!(window.size, (400.0, 500.0));
        assert_eq!((window.column_index, window.window_index), (1, 2));
        assert!(!window.is_floating);
    }

    #[test]
    fn test_apply_layout_floating_keeps_indices() {
        let mut window = create_test_window(1, 0.0, 0.0, 100.0, 200.0);
        window.column_index = 4;
        window.window_index = 1;
        window.apply_layout(&ipc_layout(None, Some((5.0, 6.0)), (300.0, 300.0)));

        assert!(window.is_floating);
        assert_eq!(window.pos, Some((5.0, 6.0)));
        assert_eq!((window.column_index, window.window_index), (4, 1));
    }

    #[test]
    fn test_to_zero_based_indices_normal_conversion() {
        assert_eq!(to_zero_based_indices(1, 1, 100), (0, 0));
        assert_eq!(to_zero_based_indices(2, 1, 100), (1, 0));
        assert_eq!(to_zero_based_indices(1, 2, 100), (0, 1));
        assert_eq!(to_zero_based_indices(5, 3, 100), (4, 2));
    }

    #[test]
    fn test_to_zero_based_indices_zero_handling() {
        assert_eq!(to_zero_based_indices(0, 1, 100), (0, 0));
        assert_eq!(to_zero_based_indices(1, 0, 100), (0, 0));
        assert_eq!(to_zero_based_indices(0, 0, 100), (0, 0));
    }

    #[test]
    fn test_to_zero_based_indices_max_value() {
        assert_eq!(
            to_zero_based_indices(usize::MAX, 1, 100),
            (usize::MAX - 1, 0)
        );
        assert_eq!(
            to_zero_based_indices(1, usize::MAX, 100),
            (0, usize::MAX - 1)
        );
        assert_eq!(
            to_zero_based_indices(usize::MAX, usize::MAX, 100),
            (usize::MAX - 1, usize::MAX - 1)
        );
    }

    #[test]
    fn test_to_zero_based_indices_large_values() {
        assert_eq!(to_zero_based_indices(1000, 500, 100), (999, 499));
        assert_eq!(to_zero_based_indices(999999, 123456, 100), (999998, 123455));
    }

    fn focused_test_window(id: u64) -> Window {
        Window {
            is_focused: true,
            ..create_test_window(id, 0.0, 0.0, 100.0, 200.0)
        }
    }

    #[test]
    fn test_apply_window_change_new_window_on_active_workspace() {
        let mut state = MinimapState::new();
        state.set_active_workspace(1);

        let change = state.apply_window_change(create_test_window(5, 0.0, 0.0, 1.0, 1.0), Some(1));

        assert_eq!(
            change,
            WindowChange {
                is_new: true,
                on_active_workspace: true
            }
        );
        assert!(state.workspaces[&1].windows.contains_key(&5));
    }

    #[test]
    fn test_apply_window_change_existing_window_is_not_new() {
        let mut state = MinimapState::new();
        state.set_active_workspace(1);
        state.apply_window_change(create_test_window(5, 0.0, 0.0, 1.0, 1.0), Some(1));

        let change =
            state.apply_window_change(create_test_window(5, 0.0, 0.0, 300.0, 400.0), Some(1));

        assert!(!change.is_new);
        assert!(change.on_active_workspace);
        assert_eq!(state.workspaces[&1].windows[&5].size, (300.0, 400.0));
    }

    #[test]
    fn test_apply_window_change_on_inactive_workspace() {
        let mut state = MinimapState::new();
        state.set_active_workspace(1);

        let change = state.apply_window_change(create_test_window(5, 0.0, 0.0, 1.0, 1.0), Some(2));

        assert!(change.is_new);
        assert!(!change.on_active_workspace);
    }

    #[test]
    fn test_apply_window_change_moves_between_workspaces() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(5, 0.0, 0.0, 1.0, 1.0));
        state.upsert_window(1, create_test_window(6, 0.0, 0.0, 1.0, 1.0));
        state.set_active_workspace(2);

        let change = state.apply_window_change(create_test_window(5, 0.0, 0.0, 1.0, 1.0), Some(2));

        assert!(change.is_new);
        assert!(change.on_active_workspace);
        assert!(!state.workspaces[&1].windows.contains_key(&5));
        assert!(state.workspaces[&1].windows.contains_key(&6));
        assert!(state.workspaces[&2].windows.contains_key(&5));
    }

    #[test]
    fn test_apply_window_change_creates_unknown_workspace_with_its_id() {
        let mut state = MinimapState::new();

        state.apply_window_change(create_test_window(5, 0.0, 0.0, 1.0, 1.0), Some(9));

        assert_eq!(state.workspaces[&9].id, 9);
    }

    #[test]
    fn test_apply_window_change_without_workspace_is_not_stored() {
        let mut state = MinimapState::new();
        state.set_active_workspace(1);

        let change = state.apply_window_change(focused_test_window(5), None);

        assert!(!change.is_new);
        assert!(!change.on_active_workspace);
        assert!(state.find_window(5).is_none());
        // Focus is still tracked.
        assert_eq!(state.focused_window_id, Some(5));
    }

    #[test]
    fn test_apply_window_change_focused_window_takes_focus() {
        let mut state = MinimapState::new();
        state.apply_window_change(focused_test_window(5), Some(1));

        state.apply_window_change(focused_test_window(6), Some(1));

        assert_eq!(state.focused_window_id, Some(6));
        assert!(!state.workspaces[&1].windows[&5].is_focused);
        assert!(state.workspaces[&1].windows[&6].is_focused);
    }

    #[test]
    fn test_apply_window_change_unfocused_keeps_focus() {
        let mut state = MinimapState::new();
        state.apply_window_change(focused_test_window(5), Some(1));

        state.apply_window_change(create_test_window(6, 0.0, 0.0, 1.0, 1.0), Some(1));

        assert_eq!(state.focused_window_id, Some(5));
    }

    #[test]
    fn test_apply_layouts_patches_known_windows() {
        let mut state = MinimapState::new();
        state.upsert_window(1, create_test_window(5, 0.0, 0.0, 1.0, 1.0));

        state.apply_layouts(&[
            (
                5,
                ipc_layout(Some((2, 1)), Some((7.0, 8.0)), (300.0, 400.0)),
            ),
            (99, ipc_layout(Some((1, 1)), None, (1.0, 1.0))),
        ]);

        let window = &state.workspaces[&1].windows[&5];
        assert_eq!(window.pos, Some((7.0, 8.0)));
        assert_eq!(window.size, (300.0, 400.0));
        assert_eq!(window.column_index, 1);
        assert!(state.find_window(99).is_none());
    }

    #[test]
    fn test_set_workspace_active_window() {
        let mut state = MinimapState::new();
        state.set_active_workspace(1);

        state.set_workspace_active_window(1, Some(5));
        state.set_workspace_active_window(2, Some(6));

        assert_eq!(state.workspaces[&1].active_window_id, Some(5));
        assert!(!state.workspaces.contains_key(&2));
    }
}
