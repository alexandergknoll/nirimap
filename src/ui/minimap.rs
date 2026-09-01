use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk4::cairo::{Context, Operator};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea};

use super::decorations::{draw_window_decorations, IconCache};
use crate::config::{Color, Config, DisplayConfig, WorkspaceMode};
use crate::state::{MinimapState, Window, Workspace};

/// Outer padding around the minimap content, in minimap pixels.
const PADDING: f64 = 4.0;

#[derive(Clone)]
pub struct MinimapWidget {
    drawing_area: DrawingArea,
    state: Rc<RefCell<MinimapState>>,
    config: Rc<RefCell<Config>>,
    window: Rc<RefCell<Option<ApplicationWindow>>>,
    hide_timeout_id: Rc<Cell<Option<glib::SourceId>>>,
    /// Last window id that triggered a show on focus change
    last_shown_focus_id: Rc<Cell<Option<u64>>>,
    /// Resolved app icons; cleared on config reload
    icon_cache: Rc<RefCell<IconCache>>,
}

impl MinimapWidget {
    pub fn new(config: Rc<RefCell<Config>>) -> Self {
        let drawing_area = DrawingArea::new();
        // Tag for the transparency CSS (see layer.rs).
        drawing_area.add_css_class("nirimap-canvas");
        let state = Rc::new(RefCell::new(MinimapState::new()));

        let height = config.borrow().display.height as i32;
        drawing_area.set_content_height(height);
        drawing_area.set_content_width(height); // square until content sizes it

        let widget = Self {
            drawing_area,
            state,
            config,
            window: Rc::new(RefCell::new(None)),
            hide_timeout_id: Rc::new(Cell::new(None)),
            last_shown_focus_id: Rc::new(Cell::new(None)),
            icon_cache: Rc::new(RefCell::new(IconCache::new())),
        };

        widget.setup_draw_handler();
        widget
    }

    /// Attach the parent window (used for resizing and visibility).
    pub fn set_window(&self, window: ApplicationWindow) {
        if !self.config.borrow().behavior.always_visible {
            window.set_visible(false);
        }
        *self.window.borrow_mut() = Some(window);
    }

    /// Show, scheduling auto-hide unless `always_visible`.
    pub fn show(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(true);
        }

        if !self.config.borrow().behavior.always_visible {
            self.schedule_hide();
        }
    }

    /// Show if focus moved to a different window than the one that last
    /// triggered a show. Returns whether it was shown.
    ///
    /// With `show_for_floating_windows` off, floating windows neither show the
    /// minimap nor advance `last_shown_focus_id`, so returning focus from a
    /// popup to the previous tile doesn't re-trigger a show.
    pub fn show_on_focus_change(&self, window_id: Option<u64>) -> bool {
        let last_id = self.last_shown_focus_id.get();

        if window_id == last_id {
            return false;
        }

        if !self.config.borrow().behavior.show_for_floating_windows {
            let is_floating = window_id
                .and_then(|id| self.state.borrow().find_window(id).map(|w| w.is_floating))
                .unwrap_or(false);
            if is_floating {
                return false;
            }
        }

        self.last_shown_focus_id.set(window_id);
        self.show();
        true
    }

    /// Show for a newly spawned window, respecting `show_for_floating_windows`.
    pub fn show_for_new_window(&self, is_floating: bool) {
        if is_floating && !self.config.borrow().behavior.show_for_floating_windows {
            return;
        }
        self.show();
    }

    pub fn hide(&self) {
        self.cancel_hide_timeout();

        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(false);
        }
    }

    fn schedule_hide(&self) {
        self.cancel_hide_timeout();

        let timeout_ms = self.config.borrow().behavior.hide_timeout_ms;
        let window = self.window.clone();
        let timeout_id_cell = self.hide_timeout_id.clone();

        let source_id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(timeout_ms as u64),
            move || {
                if let Some(win) = window.borrow().as_ref() {
                    win.set_visible(false);
                }
                timeout_id_cell.set(None);
            },
        );

        self.hide_timeout_id.set(Some(source_id));
    }

    fn cancel_hide_timeout(&self) {
        if let Some(source_id) = self.hide_timeout_id.take() {
            source_id.remove();
        }
    }

    pub fn reload_config(&self) {
        match Config::load() {
            Ok(new_config) => {
                *self.config.borrow_mut() = new_config;

                // So theme_override changes take effect.
                self.icon_cache.borrow_mut().clear();

                self.update_size();
                self.drawing_area.queue_draw();

                tracing::info!("Configuration reloaded");
            }
            Err(e) => {
                tracing::error!("Failed to reload configuration: {}", e);
            }
        }
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.drawing_area
    }

    /// Mutate the state, then resize and redraw.
    pub fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut MinimapState),
    {
        f(&mut self.state.borrow_mut());
        self.update_size();
        self.drawing_area.queue_draw();
    }

    /// Resize the widget and window to fit the current state.
    fn update_size(&self) {
        let state = self.state.borrow();
        let config = self.config.borrow();

        let (max_width, max_height) = self.get_monitor_caps();
        let viewport_width = monitor_logical_width();
        let dims = compute_widget_dimensions(
            &state,
            &config.display,
            config.appearance.workspace_gap,
            max_width,
            max_height,
            viewport_width,
        );

        let final_width = dims.width.ceil() as i32;
        let final_height = dims.height.ceil() as i32;

        self.drawing_area.set_content_width(final_width);
        self.drawing_area.set_content_height(final_height);
        if let Some(window) = self.window.borrow().as_ref() {
            window.set_default_width(final_width);
            window.set_default_height(final_height);
        }
    }

    /// Widget (max_width, max_height) from the monitor size and config percentages.
    fn get_monitor_caps(&self) -> (f64, f64) {
        let display_cfg = &self.config.borrow().display;
        let max_width_percent = display_cfg.max_width_percent;
        let max_height_percent = display_cfg.max_height_percent;

        if let Some(display) = gtk4::gdk::Display::default() {
            if let Some(monitor) = display.monitors().item(0) {
                if let Some(monitor) = monitor.downcast_ref::<gtk4::gdk::Monitor>() {
                    let geometry = monitor.geometry();
                    let w = geometry.width() as f64 * max_width_percent;
                    let h = geometry.height() as f64 * max_height_percent;
                    return (w, h);
                }
            }
        }

        // No monitor info: assume 1080p.
        (1920.0 * max_width_percent, 1080.0 * max_height_percent)
    }

    fn setup_draw_handler(&self) {
        let state = self.state.clone();
        let config = self.config.clone();
        let icon_cache = self.icon_cache.clone();

        self.drawing_area
            .set_draw_func(move |area, cr, width, height| {
                let cfg = config.borrow();
                let viewport_width = monitor_logical_width();
                draw_minimap(
                    cr,
                    width,
                    height,
                    &state.borrow(),
                    &cfg,
                    viewport_width,
                    &mut icon_cache.borrow_mut(),
                    area.scale_factor(),
                );
            });
    }
}

/// Monitor logical width, used as the workspace viewport width.
///
/// Niri's viewport equals the output's logical width. Outputs aren't queried
/// via IPC, so the GTK monitor geometry stands in — correct for the
/// single-output case (see "Known limitations" in the README).
fn monitor_logical_width() -> f64 {
    if let Some(display) = gtk4::gdk::Display::default() {
        if let Some(monitor) = display.monitors().item(0) {
            if let Some(monitor) = monitor.downcast_ref::<gtk4::gdk::Monitor>() {
                return monitor.geometry().width() as f64;
            }
        }
    }
    1920.0
}

/// Per-workspace geometry computed from its tiled windows.
struct WorkspaceLayout<'a> {
    workspace: &'a Workspace,
    /// Tiled windows grouped by column, sorted by window_index.
    columns: BTreeMap<usize, Vec<&'a Window>>,
    /// X of each column in workspace coords.
    column_x_positions: Vec<f64>,
    /// Width of the scrolling layout.
    total_width: f64,
    /// Tallest column.
    max_height: f64,
    /// Workspace-x of the viewport's left edge; rows are drawn so this lands at
    /// the shared screen anchor. See `build_workspace_layout` for how it's derived.
    align_x: f64,
    /// Left extent in viewport-relative coords: `-align_x`.
    anchored_left: f64,
    /// Right extent in viewport-relative coords: `total_width - align_x`.
    anchored_right: f64,
    has_tiled: bool,
}

/// Workspaces shown in `all` mode: those with windows, plus the focused one.
/// This hides Niri's trailing empty placeholder workspace unless the user is on it.
fn all_mode_rows(state: &MinimapState, viewport_width: f64) -> Vec<WorkspaceLayout<'_>> {
    let active_id = state.active_workspace_id;
    state
        .workspaces_sorted()
        .into_iter()
        .filter(|ws| !ws.windows.is_empty() || Some(ws.id) == active_id)
        .map(|ws| build_workspace_layout(ws, viewport_width))
        .collect()
}

/// Build the layout for a single workspace (tiled windows only).
fn build_workspace_layout(workspace: &Workspace, viewport_width: f64) -> WorkspaceLayout<'_> {
    let mut columns: BTreeMap<usize, Vec<&Window>> = BTreeMap::new();
    for window in workspace.windows.values() {
        if !window.is_floating {
            columns.entry(window.column_index).or_default().push(window);
        }
    }
    for windows in columns.values_mut() {
        windows.sort_by_key(|w| w.window_index);
    }

    let mut column_widths: Vec<f64> = Vec::new();
    let mut column_heights: Vec<f64> = Vec::new();
    if let Some(&max_col) = columns.keys().max() {
        for col_idx in 0..=max_col {
            if let Some(windows) = columns.get(&col_idx) {
                let w = windows.iter().map(|w| w.size.0).fold(0.0_f64, f64::max);
                let h: f64 = windows.iter().map(|w| w.size.1).sum();
                column_widths.push(w);
                column_heights.push(h);
            } else {
                column_widths.push(0.0);
                column_heights.push(0.0);
            }
        }
    }

    let mut column_x_positions = Vec::with_capacity(column_widths.len());
    let mut x = 0.0_f64;
    for &w in &column_widths {
        column_x_positions.push(x);
        x += w;
    }
    let total_width: f64 = column_widths.iter().sum();
    let max_height = column_heights.iter().fold(0.0_f64, |a, &b| a.max(b));

    // Viewport offset. Niri reports `pos` for tiles in the active workspace's
    // viewport, so `column_x - pos.x` gives the exact offset (possibly negative,
    // e.g. a shrunk window right-aligned in the viewport). Background workspaces
    // have no `pos`: content that fits is pinned at 0; otherwise approximate
    // with the last-focused window's column, clamped so right-edge content
    // stays right-aligned.
    let pos_offset = workspace
        .windows
        .values()
        .filter(|w| !w.is_floating)
        .find_map(|w| {
            let (px, _) = w.pos?;
            let col_x = column_x_positions.get(w.column_index).copied()?;
            Some(col_x - px)
        });
    let align_x = if let Some(offset) = pos_offset {
        offset
    } else if total_width <= viewport_width {
        0.0
    } else {
        let max_offset = total_width - viewport_width;
        workspace
            .active_window_id
            .and_then(|id| workspace.windows.get(&id))
            .filter(|w| !w.is_floating)
            .and_then(|w| column_x_positions.get(w.column_index).copied())
            .unwrap_or(0.0)
            .clamp(0.0, max_offset)
    };

    let has_tiled = !columns.is_empty();
    let anchored_left = -align_x;
    let anchored_right = total_width - align_x;

    WorkspaceLayout {
        workspace,
        columns,
        column_x_positions,
        total_width,
        max_height,
        align_x,
        anchored_left,
        anchored_right,
        has_tiled,
    }
}

struct WidgetDimensions {
    width: f64,
    height: f64,
}

/// Geometry shared by every row in `all` mode: one scale, and one screen x
/// where each workspace's viewport left edge lands so viewports line up.
struct AllModeGeometry {
    widget_width: f64,
    widget_height: f64,
    row_height: f64,
    scale: f64,
    /// Screen x of each row's viewport left edge.
    viewport_anchor_x: f64,
}

fn compute_all_mode_geometry(
    rows: &[WorkspaceLayout<'_>],
    display: &DisplayConfig,
    workspace_gap: f64,
    max_width: f64,
    max_height: f64,
    viewport_width: f64,
) -> AllModeGeometry {
    let row_height_cfg = display.height as f64;
    let min_widget_width = row_height_cfg;

    let n = rows.len().max(1) as f64;
    let total_gap = (n - 1.0).max(0.0) * workspace_gap;

    let ideal_height = n * row_height_cfg + total_gap + PADDING * 2.0;
    let min_height = row_height_cfg + PADDING * 2.0;
    let widget_height = ideal_height.min(max_height.max(min_height)).max(min_height);

    let available = widget_height - PADDING * 2.0 - total_gap;
    let row_height = (available / n).max(1.0);

    // Shared scale: fit the tallest workspace's column height into row_height.
    let global_max_height = rows
        .iter()
        .filter(|l| l.has_tiled)
        .map(|l| l.max_height)
        .fold(0.0_f64, f64::max);
    let scale = if global_max_height > 0.0 {
        row_height / global_max_height
    } else {
        0.0
    };

    // Content extents across all rows, in viewport-relative coords.
    let combined_left = rows
        .iter()
        .filter(|l| l.has_tiled)
        .map(|l| l.anchored_left)
        .fold(f64::INFINITY, f64::min);
    let combined_right = rows
        .iter()
        .filter(|l| l.has_tiled)
        .map(|l| l.anchored_right)
        .fold(f64::NEG_INFINITY, f64::max);
    let has_content = combined_left.is_finite() && combined_right.is_finite();

    let (scaled_content_width, ideal_anchor) = if has_content {
        let w = (combined_right - combined_left) * scale;
        // Leftmost content sits at x = PADDING.
        let anchor = PADDING - combined_left * scale;
        (w, anchor)
    } else {
        (0.0, PADDING)
    };

    let ideal_width = scaled_content_width + PADDING * 2.0;
    let widget_width = ideal_width.min(max_width).max(min_widget_width);

    // If content overflows the width cap, keeping the leftmost extent at PADDING
    // could push every viewport off the widget (a workspace with lots of
    // off-viewport content to its left has a large `align_x`). Center the
    // viewport instead so it's always visible.
    let inner_width = (widget_width - PADDING * 2.0).max(0.0);
    let viewport_anchor_x = if !has_content || scaled_content_width <= inner_width {
        ideal_anchor
    } else {
        let viewport_scaled = viewport_width * scale;
        PADDING + (inner_width - viewport_scaled) / 2.0
    };

    AllModeGeometry {
        widget_width,
        widget_height,
        row_height,
        scale,
        viewport_anchor_x,
    }
}

/// Widget size for the current state.
fn compute_widget_dimensions(
    state: &MinimapState,
    display: &DisplayConfig,
    workspace_gap: f64,
    max_width: f64,
    max_height: f64,
    viewport_width: f64,
) -> WidgetDimensions {
    let row_height_cfg = display.height as f64;
    let min_widget_width = row_height_cfg;

    match display.workspace_mode {
        WorkspaceMode::Current => {
            let widget_height = row_height_cfg;
            let row_height = (widget_height - PADDING * 2.0).max(0.0);
            let scaled_w = state
                .active_workspace()
                .map(|ws| {
                    row_scaled_width_centered(
                        &build_workspace_layout(ws, viewport_width),
                        row_height,
                    )
                })
                .unwrap_or(0.0);

            let ideal_width = scaled_w + PADDING * 2.0;
            let width = ideal_width.min(max_width).max(min_widget_width);

            WidgetDimensions {
                width,
                height: widget_height,
            }
        }
        WorkspaceMode::All => {
            let rows = all_mode_rows(state, viewport_width);
            let geom = compute_all_mode_geometry(
                &rows,
                display,
                workspace_gap,
                max_width,
                max_height,
                viewport_width,
            );

            WidgetDimensions {
                width: geom.widget_width,
                height: geom.widget_height,
            }
        }
    }
}

/// Scaled width of a row in `current` mode at the given inner row height.
fn row_scaled_width_centered(layout: &WorkspaceLayout<'_>, row_inner_height: f64) -> f64 {
    if layout.total_width <= 0.0 || layout.max_height <= 0.0 || row_inner_height <= 0.0 {
        return 0.0;
    }
    let scale = row_inner_height / layout.max_height;
    layout.total_width * scale
}

#[allow(clippy::too_many_arguments)]
fn draw_minimap(
    cr: &Context,
    width: i32,
    height: i32,
    state: &MinimapState,
    config: &Config,
    viewport_width: f64,
    icon_cache: &mut IconCache,
    widget_scale: i32,
) {
    let display = &config.display;
    let appearance = &config.appearance;
    let width = width as f64;
    let height = height as f64;

    // Clear to transparent.
    cr.set_operator(Operator::Clear);
    cr.paint().ok();
    cr.set_operator(Operator::Over);

    // Optional background (transparent by default).
    if appearance.background_opacity > 0.0 {
        if let Some(bg_color) = Color::from_hex(&appearance.background) {
            cr.set_source_rgba(
                bg_color.r,
                bg_color.g,
                bg_color.b,
                appearance.background_opacity,
            );
            rounded_rectangle(cr, 0.0, 0.0, width, height, appearance.border_radius * 2.0);
            cr.fill().ok();
        }
    }

    let inner_width = (width - PADDING * 2.0).max(0.0);

    match display.workspace_mode {
        WorkspaceMode::Current => {
            let Some(workspace) = state.active_workspace() else {
                return;
            };
            let layout = build_workspace_layout(workspace, viewport_width);
            if layout.total_width <= 0.0 || layout.max_height <= 0.0 {
                return;
            }
            let row_inner_height = (height - PADDING * 2.0).max(0.0);
            draw_workspace_row_centered(
                cr,
                &layout,
                PADDING,
                PADDING,
                inner_width,
                row_inner_height,
                config,
                icon_cache,
                widget_scale,
            );
        }
        WorkspaceMode::All => {
            let rows = all_mode_rows(state, viewport_width);
            if rows.is_empty() {
                return;
            }

            // Recompute with the actual widget size as the cap, matching update_size().
            let geom = compute_all_mode_geometry(
                &rows,
                display,
                appearance.workspace_gap,
                width,
                height,
                viewport_width,
            );

            let active_border = Color::from_hex(&appearance.active_workspace_border_color)
                .unwrap_or(Color {
                    r: 0.54,
                    g: 0.71,
                    b: 0.98,
                    a: 1.0,
                });

            let mut y = PADDING;
            for layout in &rows {
                // Active workspace border.
                if layout.workspace.is_active && appearance.active_workspace_border_width > 0.0 {
                    cr.set_source_rgba(
                        active_border.r,
                        active_border.g,
                        active_border.b,
                        active_border.a,
                    );
                    cr.set_line_width(appearance.active_workspace_border_width);
                    let inset = appearance.active_workspace_border_width / 2.0;
                    rounded_rectangle(
                        cr,
                        PADDING + inset,
                        y + inset,
                        (inner_width - inset * 2.0).max(0.0),
                        (geom.row_height - inset * 2.0).max(0.0),
                        appearance.border_radius,
                    );
                    cr.stroke().ok();
                }

                if layout.has_tiled && geom.scale > 0.0 {
                    draw_workspace_row_viewport(
                        cr,
                        layout,
                        PADDING,
                        y,
                        inner_width,
                        geom.row_height,
                        geom.scale,
                        geom.viewport_anchor_x,
                        config,
                        icon_cache,
                        widget_scale,
                    );
                }

                y += geom.row_height + appearance.workspace_gap;
            }
        }
    }
}

/// `current` mode: draw a workspace's tiled windows scaled to `row_height` and
/// horizontally centered in the row.
#[allow(clippy::too_many_arguments)]
fn draw_workspace_row_centered(
    cr: &Context,
    layout: &WorkspaceLayout<'_>,
    offset_x: f64,
    offset_y: f64,
    row_width: f64,
    row_height: f64,
    config: &Config,
    icon_cache: &mut IconCache,
    widget_scale: i32,
) {
    let appearance = &config.appearance;
    if layout.total_width <= 0.0 || layout.max_height <= 0.0 || row_height <= 0.0 {
        return;
    }

    let scale = row_height / layout.max_height;
    let scaled_width = layout.total_width * scale;
    let x_origin = offset_x + (row_width - scaled_width).max(0.0) / 2.0;
    let y_origin = offset_y;

    let window_color = Color::from_hex(&appearance.window_color).unwrap_or(Color {
        r: 0.27,
        g: 0.28,
        b: 0.35,
        a: 1.0,
    });
    let focused_color = Color::from_hex(&appearance.focused_color).unwrap_or(Color {
        r: 0.54,
        g: 0.71,
        b: 0.98,
        a: 1.0,
    });
    let border_color = Color::from_hex(&appearance.border_color).unwrap_or(Color {
        r: 0.42,
        g: 0.44,
        b: 0.53,
        a: 1.0,
    });

    let gap = appearance.gap;
    let half_gap = gap / 2.0;

    for (&col_idx, windows) in &layout.columns {
        let col_x = layout
            .column_x_positions
            .get(col_idx)
            .copied()
            .unwrap_or(0.0);
        let mut y_pos = 0.0;

        for window in windows {
            let x = x_origin + col_x * scale;
            let y = y_origin + y_pos * scale;
            let w = window.size.0 * scale;
            let h = window.size.1 * scale;

            y_pos += window.size.1;

            let x = x + half_gap;
            let y = y + half_gap;
            let w = (w - gap).max(1.0);
            let h = (h - gap).max(1.0);

            if w < 1.0 || h < 1.0 {
                continue;
            }

            let (fill_color, fill_alpha) = if window.is_focused {
                (&focused_color, appearance.focused_opacity)
            } else {
                (&window_color, appearance.window_opacity)
            };

            if fill_alpha > 0.0 {
                cr.set_source_rgba(fill_color.r, fill_color.g, fill_color.b, fill_alpha);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.fill().ok();
            }

            if appearance.border_width > 0.0 {
                cr.set_source_rgba(
                    border_color.r,
                    border_color.g,
                    border_color.b,
                    border_color.a,
                );
                cr.set_line_width(appearance.border_width);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.stroke().ok();
            }

            draw_window_decorations(cr, window, x, y, w, h, config, icon_cache, widget_scale);
        }
    }

    // Floating windows aren't drawn: Niri IPC doesn't expose the viewport offset,
    // so their placement would be unreliable (issue #6).
}

/// `all` mode: draw a workspace's tiled windows at the shared `scale`, shifted so
/// its viewport left edge lands at `viewport_anchor_x` (Overview-style), and
/// clipped to the row.
#[allow(clippy::too_many_arguments)]
fn draw_workspace_row_viewport(
    cr: &Context,
    layout: &WorkspaceLayout<'_>,
    offset_x: f64,
    offset_y: f64,
    row_width: f64,
    row_height: f64,
    scale: f64,
    viewport_anchor_x: f64,
    config: &Config,
    icon_cache: &mut IconCache,
    widget_scale: i32,
) {
    let appearance = &config.appearance;
    if !layout.has_tiled || scale <= 0.0 || row_width <= 0.0 || row_height <= 0.0 {
        return;
    }

    let window_color = Color::from_hex(&appearance.window_color).unwrap_or(Color {
        r: 0.27,
        g: 0.28,
        b: 0.35,
        a: 1.0,
    });
    let focused_color = Color::from_hex(&appearance.focused_color).unwrap_or(Color {
        r: 0.54,
        g: 0.71,
        b: 0.98,
        a: 1.0,
    });
    let border_color = Color::from_hex(&appearance.border_color).unwrap_or(Color {
        r: 0.42,
        g: 0.44,
        b: 0.53,
        a: 1.0,
    });

    let gap = appearance.gap;
    let half_gap = gap / 2.0;

    // Screen x of workspace-x = 0.
    let row_x_origin = viewport_anchor_x - layout.align_x * scale;
    let y_origin = offset_y;

    // Clip so off-viewport content doesn't spill into neighbouring rows.
    cr.save().ok();
    cr.rectangle(offset_x, offset_y, row_width, row_height);
    cr.clip();

    for (&col_idx, windows) in &layout.columns {
        let col_x = layout
            .column_x_positions
            .get(col_idx)
            .copied()
            .unwrap_or(0.0);
        let mut y_pos = 0.0;

        for window in windows {
            let x = row_x_origin + col_x * scale;
            let y = y_origin + y_pos * scale;
            let w = window.size.0 * scale;
            let h = window.size.1 * scale;

            y_pos += window.size.1;

            let x = x + half_gap;
            let y = y + half_gap;
            let w = (w - gap).max(1.0);
            let h = (h - gap).max(1.0);

            if w < 1.0 || h < 1.0 {
                continue;
            }

            let (fill_color, fill_alpha) = if window.is_focused {
                (&focused_color, appearance.focused_opacity)
            } else {
                (&window_color, appearance.window_opacity)
            };

            if fill_alpha > 0.0 {
                cr.set_source_rgba(fill_color.r, fill_color.g, fill_color.b, fill_alpha);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.fill().ok();
            }

            if appearance.border_width > 0.0 {
                cr.set_source_rgba(
                    border_color.r,
                    border_color.g,
                    border_color.b,
                    border_color.a,
                );
                cr.set_line_width(appearance.border_width);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.stroke().ok();
            }

            draw_window_decorations(cr, window, x, y, w, h, config, icon_cache, widget_scale);
        }
    }

    cr.restore().ok();
}

/// Trace a rounded-rectangle path (no fill or stroke).
fn rounded_rectangle(cr: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);

    cr.new_path();
    cr.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}
