use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk4::cairo::{Context, Operator};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea};

use super::decorations::{draw_window_decorations, IconCache};
use super::draw::{DrawCtx, Palette};
use crate::config::{Config, DisplayConfig, WorkspaceMode};
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
    /// Running fade-in/out animation on the window
    fade: Rc<Cell<Option<Fade>>>,
    /// Last window id that triggered a show on focus change
    last_shown_focus_id: Rc<Cell<Option<u64>>>,
    /// Resolved app icons; cleared on config reload
    icon_cache: Rc<RefCell<IconCache>>,
    /// Colours parsed from `config`; rebuilt on config reload
    palette: Rc<Cell<Palette>>,
}

impl MinimapWidget {
    pub fn new(config: Rc<RefCell<Config>>) -> Self {
        let drawing_area = DrawingArea::new();
        // Tag for the transparency CSS (see layer.rs).
        drawing_area.add_css_class("nirimap-canvas");
        let state = Rc::new(RefCell::new(MinimapState::new()));

        let height = config.borrow().display.height as i32;
        let palette = Palette::from_config(&config.borrow());
        drawing_area.set_content_height(height);
        drawing_area.set_content_width(height); // square until content sizes it

        let widget = Self {
            drawing_area,
            state,
            config,
            window: Rc::new(RefCell::new(None)),
            hide_timeout_id: Rc::new(Cell::new(None)),
            fade: Rc::new(Cell::new(None)),
            last_shown_focus_id: Rc::new(Cell::new(None)),
            icon_cache: Rc::new(RefCell::new(IconCache::new())),
            palette: Rc::new(Cell::new(palette)),
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

    /// Show (fading in over `fade_in_ms`), scheduling auto-hide unless
    /// `always_visible`.
    pub fn show(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            if !window.is_visible() {
                window.set_opacity(0.0);
                window.set_visible(true);
            }
        }
        let fade_in_ms = self.config.borrow().behavior.fade_in_ms;
        fade_window(&self.window, &self.fade, 1.0, fade_in_ms, false);

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

    /// Hide immediately, without fading.
    pub fn hide(&self) {
        self.cancel_hide_timeout();
        cancel_fade(&self.fade);

        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(false);
        }
    }

    fn schedule_hide(&self) {
        self.cancel_hide_timeout();

        let (timeout_ms, fade_out_ms) = {
            let behavior = &self.config.borrow().behavior;
            (behavior.hide_timeout_ms, behavior.fade_out_ms)
        };
        let window = self.window.clone();
        let timeout_id_cell = self.hide_timeout_id.clone();
        let fade = self.fade.clone();

        let source_id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(timeout_ms as u64),
            move || {
                timeout_id_cell.set(None);
                fade_window(&window, &fade, 0.0, fade_out_ms, true);
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
                self.palette.set(Palette::from_config(&new_config));
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

    /// Mutate the state, then resize and redraw. Returns the closure's result.
    pub fn update_state<R>(&self, f: impl FnOnce(&mut MinimapState) -> R) -> R {
        let result = f(&mut self.state.borrow_mut());
        self.update_size();
        self.drawing_area.queue_draw();
        result
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
        let palette = self.palette.clone();

        self.drawing_area
            .set_draw_func(move |area, cr, width, height| {
                let config = config.borrow();
                let mut ctx = DrawCtx {
                    cr,
                    config: &config,
                    palette: palette.get(),
                    icon_cache: &mut icon_cache.borrow_mut(),
                    widget_scale: area.scale_factor(),
                };
                draw_minimap(
                    &mut ctx,
                    width,
                    height,
                    &state.borrow(),
                    monitor_logical_width(),
                );
            });
    }
}

/// A running opacity animation on the minimap window.
struct Fade {
    tick_id: gtk4::TickCallbackId,
    target: f64,
}

/// Animate the window's opacity to `target` over `duration_ms` (ease-out),
/// hiding the window at the end if `hide_when_done`. A fade already heading to
/// `target` is left running (shows arrive in bursts); any other fade is
/// replaced, starting from the current opacity, so a show during a fade-out
/// reverses it smoothly. A zero duration applies the end state immediately.
fn fade_window(
    window: &Rc<RefCell<Option<ApplicationWindow>>>,
    fade: &Rc<Cell<Option<Fade>>>,
    target: f64,
    duration_ms: u32,
    hide_when_done: bool,
) {
    if let Some(running) = fade.take() {
        if running.target == target {
            fade.set(Some(running));
            return;
        }
        running.tick_id.remove();
    }
    let Some(window) = window.borrow().clone() else {
        return;
    };

    let from = window.opacity();
    if duration_ms == 0 || from == target {
        window.set_opacity(target);
        if hide_when_done {
            window.set_visible(false);
        }
        return;
    }

    let duration_ms = duration_ms as f64;
    // Timed from the first frame, so a fade-in starts once the surface is mapped.
    let started_us = Cell::new(None::<i64>);
    let fade_cell = fade.clone();
    let tick_id = window.add_tick_callback(move |window, clock| {
        let now_us = clock.frame_time();
        let start_us = started_us.get().unwrap_or(now_us);
        started_us.set(Some(start_us));
        let elapsed_ms = (now_us - start_us) as f64 / 1000.0;

        window.set_opacity(fade_opacity(from, target, elapsed_ms, duration_ms));
        if elapsed_ms < duration_ms {
            return glib::ControlFlow::Continue;
        }
        if hide_when_done {
            window.set_visible(false);
        }
        // Returning Break removes the callback; forget its id so it isn't removed again.
        fade_cell.set(None);
        glib::ControlFlow::Break
    });
    fade.set(Some(Fade { tick_id, target }));
}

fn cancel_fade(fade: &Cell<Option<Fade>>) {
    if let Some(running) = fade.take() {
        running.tick_id.remove();
    }
}

/// Opacity `elapsed_ms` into a `duration_ms` fade from `from` to `to`, with
/// an ease-out cubic curve (fast start, gentle landing).
fn fade_opacity(from: f64, to: f64, elapsed_ms: f64, duration_ms: f64) -> f64 {
    let t = if duration_ms > 0.0 {
        (elapsed_ms / duration_ms).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let eased = 1.0 - (1.0 - t).powi(3);
    from + (to - from) * eased
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
    /// Empty row slots above the first row (non-zero only with `center_viewport`).
    first_row_slot: f64,
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

    // With `center_viewport`, reserve `rows - 1` slots both above and below the
    // active row so it sits at the same spot whichever workspace is active, and
    // the widget height stays the same while switching.
    let (n, first_row_slot) = if display.center_viewport {
        let others = rows.len().saturating_sub(1);
        let active = rows.iter().position(|l| l.workspace.is_active).unwrap_or(0);
        (
            (2 * others + 1) as f64,
            (others - active.min(others)) as f64,
        )
    } else {
        (rows.len().max(1) as f64, 0.0)
    };
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

    let viewport_scaled = viewport_width * scale;
    let (widget_width, viewport_anchor_x) = if display.center_viewport {
        let side = if has_content {
            centered_side_extent(combined_left, combined_right, viewport_width) * scale
        } else {
            0.0
        };
        centered_width_and_anchor(viewport_scaled, side, min_widget_width, max_width)
    } else {
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
            PADDING + (inner_width - viewport_scaled) / 2.0
        };
        (widget_width, viewport_anchor_x)
    };

    AllModeGeometry {
        widget_width,
        widget_height,
        row_height,
        scale,
        viewport_anchor_x,
        first_row_slot,
    }
}

/// How far content reaches past the viewport on its wider side, in workspace
/// units, given viewport-relative extents (`left` <= 0 means content left of
/// the viewport). With `center_viewport`, both sides get this much room so all
/// content fits around a centered viewport.
fn centered_side_extent(left: f64, right: f64, viewport_width: f64) -> f64 {
    (-left).max(right - viewport_width).max(0.0)
}

/// Widget width and viewport left edge (screen x) for `center_viewport`: the
/// scaled viewport sits in the middle with `side` pixels of room on each side,
/// width clamped to [`min_width`, `max_width`].
fn centered_width_and_anchor(
    viewport_scaled: f64,
    side: f64,
    min_width: f64,
    max_width: f64,
) -> (f64, f64) {
    let ideal_width = viewport_scaled + side * 2.0 + PADDING * 2.0;
    let width = ideal_width.min(max_width).max(min_width);
    (width, (width - viewport_scaled) / 2.0)
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
            let layout = state
                .active_workspace()
                .map(|ws| build_workspace_layout(ws, viewport_width));

            let width = if display.center_viewport {
                let geom = layout.as_ref().and_then(|l| {
                    current_mode_centered(
                        l,
                        row_height,
                        viewport_width,
                        min_widget_width,
                        max_width,
                    )
                });
                geom.map_or(min_widget_width, |(width, _, _)| width)
            } else {
                let scaled_w = layout
                    .as_ref()
                    .map(|l| row_scaled_width_centered(l, row_height))
                    .unwrap_or(0.0);
                let ideal_width = scaled_w + PADDING * 2.0;
                ideal_width.min(max_width).max(min_widget_width)
            };

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

/// `current` mode with `center_viewport`: (widget width, screen x of
/// workspace-x 0, scale), or `None` when there is nothing to draw.
fn current_mode_centered(
    layout: &WorkspaceLayout<'_>,
    row_inner_height: f64,
    viewport_width: f64,
    min_width: f64,
    max_width: f64,
) -> Option<(f64, f64, f64)> {
    if !layout.has_tiled || layout.max_height <= 0.0 || row_inner_height <= 0.0 {
        return None;
    }
    let scale = row_inner_height / layout.max_height;
    let side = centered_side_extent(layout.anchored_left, layout.anchored_right, viewport_width);
    let (width, viewport_anchor_x) =
        centered_width_and_anchor(viewport_width * scale, side * scale, min_width, max_width);
    Some((width, viewport_anchor_x - layout.align_x * scale, scale))
}

fn draw_minimap(
    ctx: &mut DrawCtx<'_>,
    width: i32,
    height: i32,
    state: &MinimapState,
    viewport_width: f64,
) {
    let cr = ctx.cr;
    let appearance = &ctx.config.appearance;
    let width = width as f64;
    let height = height as f64;

    // Clear to transparent.
    cr.set_operator(Operator::Clear);
    cr.paint().ok();
    cr.set_operator(Operator::Over);

    // Optional background (transparent by default).
    if appearance.background_opacity > 0.0 {
        if let Some(bg_color) = ctx.palette.background {
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

    match ctx.config.display.workspace_mode {
        WorkspaceMode::Current => draw_current_workspace(ctx, state, width, height, viewport_width),
        WorkspaceMode::All => draw_all_workspaces(ctx, state, width, height, viewport_width),
    }
}

/// `current` mode: the active workspace's tiled windows, scaled to the widget
/// height and horizontally centered (or, with `center_viewport`, positioned so
/// the viewport is centered).
fn draw_current_workspace(
    ctx: &mut DrawCtx<'_>,
    state: &MinimapState,
    width: f64,
    height: f64,
    viewport_width: f64,
) {
    let Some(workspace) = state.active_workspace() else {
        return;
    };
    let layout = build_workspace_layout(workspace, viewport_width);
    let row_width = (width - PADDING * 2.0).max(0.0);
    let row_height = (height - PADDING * 2.0).max(0.0);
    if layout.total_width <= 0.0 || layout.max_height <= 0.0 || row_height <= 0.0 {
        return;
    }

    if ctx.config.display.center_viewport {
        // Pin the width to the actual widget width so the viewport lands in its middle.
        let Some((_, x_origin, scale)) =
            current_mode_centered(&layout, row_height, viewport_width, width, width)
        else {
            return;
        };
        let cr = ctx.cr;
        cr.save().ok();
        cr.rectangle(PADDING, PADDING, row_width, row_height);
        cr.clip();
        draw_tiled_windows(ctx, &layout, x_origin, PADDING, scale);
        cr.restore().ok();
        return;
    }

    let scale = row_height / layout.max_height;
    let scaled_width = layout.total_width * scale;
    let x_origin = PADDING + (row_width - scaled_width).max(0.0) / 2.0;
    draw_tiled_windows(ctx, &layout, x_origin, PADDING, scale);
}

/// `all` mode: one row per workspace at a shared scale, each shifted so its
/// viewport left edge lands at the same x (Overview-style).
fn draw_all_workspaces(
    ctx: &mut DrawCtx<'_>,
    state: &MinimapState,
    width: f64,
    height: f64,
    viewport_width: f64,
) {
    let cr = ctx.cr;
    let appearance = &ctx.config.appearance;

    let rows = all_mode_rows(state, viewport_width);
    if rows.is_empty() {
        return;
    }

    // Recompute with the actual widget size as the cap, matching update_size().
    let geom = compute_all_mode_geometry(
        &rows,
        &ctx.config.display,
        appearance.workspace_gap,
        width,
        height,
        viewport_width,
    );

    let active_border = ctx.palette.active_workspace_border;

    let row_width = (width - PADDING * 2.0).max(0.0);
    let mut y = PADDING + geom.first_row_slot * (geom.row_height + appearance.workspace_gap);
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
                (row_width - inset * 2.0).max(0.0),
                (geom.row_height - inset * 2.0).max(0.0),
                appearance.border_radius,
            );
            cr.stroke().ok();
        }

        if layout.has_tiled && geom.scale > 0.0 && row_width > 0.0 && geom.row_height > 0.0 {
            // Screen x of workspace-x = 0.
            let x_origin = geom.viewport_anchor_x - layout.align_x * geom.scale;

            // Clip so off-viewport content doesn't spill into neighbouring rows.
            cr.save().ok();
            cr.rectangle(PADDING, y, row_width, geom.row_height);
            cr.clip();
            draw_tiled_windows(ctx, layout, x_origin, y, geom.scale);
            cr.restore().ok();
        }

        y += geom.row_height + appearance.workspace_gap;
    }
}

/// Draw a workspace's tiled windows, with workspace coordinate (0, 0) at
/// (`x_origin`, `y_origin`) on screen.
///
/// Floating windows aren't drawn: Niri IPC doesn't expose the viewport offset,
/// so their placement would be unreliable (issue #6).
fn draw_tiled_windows(
    ctx: &mut DrawCtx<'_>,
    layout: &WorkspaceLayout<'_>,
    x_origin: f64,
    y_origin: f64,
    scale: f64,
) {
    let cr = ctx.cr;
    let appearance = &ctx.config.appearance;

    let palette = ctx.palette;

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
            // Inset by half the gap on each side; never collapse below 1px.
            let x = x_origin + col_x * scale + half_gap;
            let y = y_origin + y_pos * scale + half_gap;
            let w = (window.size.0 * scale - gap).max(1.0);
            let h = (window.size.1 * scale - gap).max(1.0);

            y_pos += window.size.1;

            let (fill_color, fill_alpha) = if window.is_focused {
                (palette.focused, appearance.focused_opacity)
            } else {
                (palette.window, appearance.window_opacity)
            };

            if fill_alpha > 0.0 {
                cr.set_source_rgba(fill_color.r, fill_color.g, fill_color.b, fill_alpha);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.fill().ok();
            }

            if appearance.border_width > 0.0 {
                cr.set_source_rgba(
                    palette.border.r,
                    palette.border.g,
                    palette.border.b,
                    palette.border.a,
                );
                cr.set_line_width(appearance.border_width);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.stroke().ok();
            }

            draw_window_decorations(ctx, window, x, y, w, h);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const VIEWPORT: f64 = 1000.0;

    fn tile(id: u64, column_index: usize, pos: Option<(f64, f64)>) -> Window {
        Window {
            id,
            pos,
            size: (500.0, 1000.0),
            column_index,
            window_index: 0,
            is_focused: false,
            is_floating: false,
            title: None,
            app_id: None,
        }
    }

    /// A workspace with three 500-wide columns (1500 total, viewport 1000),
    /// scrolled so the viewport's left edge is at column `first_visible`.
    fn workspace(id: u64, idx: u8, is_active: bool, first_visible: usize) -> Workspace {
        let windows: HashMap<u64, Window> = (0..3)
            .map(|col| {
                let wid = id * 10 + col as u64;
                let pos = is_active.then_some(((col as f64 - first_visible as f64) * 500.0, 0.0));
                (wid, tile(wid, col, pos))
            })
            .collect();
        Workspace {
            id,
            idx,
            output: None,
            windows,
            is_active,
            active_window_id: None,
        }
    }

    fn display(center_viewport: bool) -> DisplayConfig {
        DisplayConfig {
            height: 100,
            center_viewport,
            ..Default::default()
        }
    }

    fn all_mode(ws: &[Workspace], display: &DisplayConfig, max_width: f64) -> AllModeGeometry {
        let rows: Vec<_> = ws
            .iter()
            .map(|w| build_workspace_layout(w, VIEWPORT))
            .collect();
        compute_all_mode_geometry(&rows, display, 4.0, max_width, 10_000.0, VIEWPORT)
    }

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn test_fade_opacity() {
        assert_close(fade_opacity(0.0, 1.0, 0.0, 100.0), 0.0);
        assert_close(fade_opacity(0.0, 1.0, 50.0, 100.0), 0.875);
        assert_close(fade_opacity(0.0, 1.0, 100.0, 100.0), 1.0);
        assert_close(fade_opacity(0.0, 1.0, 250.0, 100.0), 1.0);
        // Fading out from a partly faded-in state.
        assert_close(fade_opacity(0.4, 0.0, 50.0, 100.0), 0.05);
        assert_close(fade_opacity(1.0, 0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_all_mode_fit_tracks_content() {
        // Scale 0.1: content is 150 px, the viewport 100 px.
        let left = all_mode(&[workspace(1, 1, true, 0)], &display(false), 10_000.0);
        assert_close(left.widget_width, 158.0);
        assert_close(left.viewport_anchor_x, PADDING);

        let right = all_mode(&[workspace(1, 1, true, 1)], &display(false), 10_000.0);
        assert_close(right.widget_width, 158.0);
        assert_close(right.viewport_anchor_x, PADDING + 50.0);
    }

    #[test]
    fn test_all_mode_centered_viewport_stays_put() {
        // 50 px of off-viewport content on one side, so 50 px of room on both.
        for first_visible in [0, 1] {
            let geom = all_mode(
                &[workspace(1, 1, true, first_visible)],
                &display(true),
                10_000.0,
            );
            assert_close(geom.widget_width, 100.0 + 2.0 * 50.0 + 2.0 * PADDING);
            assert_close(geom.viewport_anchor_x + 50.0, geom.widget_width / 2.0);
        }
    }

    #[test]
    fn test_all_mode_centered_respects_max_width() {
        let geom = all_mode(&[workspace(1, 1, true, 0)], &display(true), 150.0);
        assert_close(geom.widget_width, 150.0);
        assert_close(geom.viewport_anchor_x, 25.0);
    }

    #[test]
    fn test_all_mode_fit_height_follows_rows() {
        let ws = [workspace(1, 1, false, 0), workspace(2, 2, true, 0)];
        let geom = all_mode(&ws, &display(false), 10_000.0);
        assert_close(geom.widget_height, 2.0 * 100.0 + 4.0 + 2.0 * PADDING);
        assert_close(geom.first_row_slot, 0.0);
    }

    #[test]
    fn test_all_mode_centered_active_row_stays_put() {
        // Three rows: five slots, the active row always in the middle one.
        for active in 0..3 {
            let ws: Vec<_> = (0..3)
                .map(|i| workspace(i + 1, i as u8 + 1, i == active, 0))
                .collect();
            let geom = all_mode(&ws, &display(true), 10_000.0);
            assert_close(geom.widget_height, 5.0 * 100.0 + 4.0 * 4.0 + 2.0 * PADDING);
            assert_close(geom.first_row_slot + active as f64, 2.0);
        }
    }

    #[test]
    fn test_current_mode_centered_viewport() {
        for first_visible in [0, 1] {
            let ws = workspace(1, 1, true, first_visible);
            let layout = build_workspace_layout(&ws, VIEWPORT);
            let (width, x_origin, scale) =
                current_mode_centered(&layout, 100.0, VIEWPORT, 0.0, 10_000.0).unwrap();
            assert_close(scale, 0.1);
            assert_close(width, 100.0 + 2.0 * 50.0 + 2.0 * PADDING);
            // Viewport left edge (workspace x = align_x) sits 50 px left of centre.
            assert_close(x_origin + layout.align_x * scale, width / 2.0 - 50.0);
        }
    }
}
