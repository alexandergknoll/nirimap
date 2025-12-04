use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::cairo::{Context, Operator};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea};

use crate::config::{AppearanceConfig, Color, Config};
use crate::state::{MinimapState, Window};

/// Wrapper around DrawingArea for the minimap
#[derive(Clone)]
pub struct MinimapWidget {
    drawing_area: DrawingArea,
    state: Rc<RefCell<MinimapState>>,
    config: Rc<RefCell<Config>>,
    window: Rc<RefCell<Option<ApplicationWindow>>>,
    hide_timeout_id: Rc<Cell<Option<glib::SourceId>>>,
    /// Track the last window ID that triggered a show via focus change
    last_shown_focus_id: Rc<Cell<Option<u64>>>,
}

impl MinimapWidget {
    /// Create a new minimap widget
    pub fn new(config: Rc<RefCell<Config>>) -> Self {
        let drawing_area = DrawingArea::new();
        let state = Rc::new(RefCell::new(MinimapState::new()));

        // Start with just the height; width will be calculated
        let height = config.borrow().display.height as i32;
        drawing_area.set_content_height(height);
        drawing_area.set_content_width(height); // Start square

        let widget = Self {
            drawing_area,
            state,
            config,
            window: Rc::new(RefCell::new(None)),
            hide_timeout_id: Rc::new(Cell::new(None)),
            last_shown_focus_id: Rc::new(Cell::new(None)),
        };

        widget.setup_draw_handler();
        widget
    }

    /// Set the parent window (needed for dynamic resizing and visibility)
    pub fn set_window(&self, window: ApplicationWindow) {
        // Set initial visibility based on config
        if !self.config.borrow().behavior.always_visible {
            window.set_visible(false);
        }
        *self.window.borrow_mut() = Some(window);
    }

    /// Show the minimap (with auto-hide timeout if configured)
    pub fn show(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(true);
        }

        // If not always visible, schedule hide after timeout
        if !self.config.borrow().behavior.always_visible {
            self.schedule_hide();
        }
    }

    /// Show the minimap only if focus changed to a different window
    /// Returns true if the minimap was shown
    pub fn show_on_focus_change(&self, window_id: Option<u64>) -> bool {
        let last_id = self.last_shown_focus_id.get();

        // Only show if focus changed to a different window
        if window_id != last_id {
            self.last_shown_focus_id.set(window_id);
            self.show();
            true
        } else {
            false
        }
    }

    /// Hide the minimap
    pub fn hide(&self) {
        // Cancel any pending hide timeout
        self.cancel_hide_timeout();

        if let Some(window) = self.window.borrow().as_ref() {
            window.set_visible(false);
        }
    }

    /// Schedule hiding the minimap after the configured timeout
    fn schedule_hide(&self) {
        // Cancel any existing timeout
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

    /// Cancel any pending hide timeout
    fn cancel_hide_timeout(&self) {
        if let Some(source_id) = self.hide_timeout_id.take() {
            source_id.remove();
        }
    }

    /// Check if minimap should always be visible
    pub fn is_always_visible(&self) -> bool {
        self.config.borrow().behavior.always_visible
    }

    /// Reload the configuration from disk
    pub fn reload_config(&self) {
        match Config::load() {
            Ok(new_config) => {
                let old_height = self.config.borrow().display.height;
                let new_height = new_config.display.height;

                // Update the config
                *self.config.borrow_mut() = new_config;

                // If height changed, update the drawing area
                if old_height != new_height {
                    self.drawing_area.set_content_height(new_height as i32);
                }

                // Trigger resize and redraw
                self.update_size();
                self.drawing_area.queue_draw();

                tracing::info!("Configuration reloaded");
            }
            Err(e) => {
                tracing::error!("Failed to reload configuration: {}", e);
            }
        }
    }

    /// Get the underlying DrawingArea widget
    pub fn widget(&self) -> &DrawingArea {
        &self.drawing_area
    }

    /// Update the state, resize if needed, and trigger a redraw
    pub fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut MinimapState),
    {
        f(&mut self.state.borrow_mut());
        self.update_size();
        self.drawing_area.queue_draw();
    }

    /// Calculate and update the widget/window size based on current state
    fn update_size(&self) {
        let state = self.state.borrow();
        let config = self.config.borrow();

        let height = config.display.height as f64;
        let padding = 4.0;
        let inner_height = height - padding * 2.0;

        // Calculate workspace dimensions
        let (total_width, max_height) = calculate_workspace_dimensions(&state);

        if total_width <= 0.0 || max_height <= 0.0 {
            // No windows, use minimum size
            let min_width = height as i32;
            self.drawing_area.set_content_width(min_width);
            if let Some(window) = self.window.borrow().as_ref() {
                window.set_default_width(min_width);
            }
            return;
        }

        // Calculate scale based on fitting workspace height into minimap height
        let scale = inner_height / max_height;

        // Calculate required width
        let ideal_width = (total_width * scale + padding * 2.0).ceil();

        // Get max width from monitor
        let max_width = self.get_max_width();

        // Clamp width
        let final_width = ideal_width.min(max_width).max(height) as i32;

        // Update drawing area and window size
        self.drawing_area.set_content_width(final_width);
        if let Some(window) = self.window.borrow().as_ref() {
            window.set_default_width(final_width);
        }
    }

    /// Get the maximum allowed width based on monitor and config
    fn get_max_width(&self) -> f64 {
        let max_width_percent = self.config.borrow().display.max_width_percent;

        // Try to get monitor dimensions
        if let Some(display) = gtk4::gdk::Display::default() {
            if let Some(monitor) = display.monitors().item(0) {
                if let Some(monitor) = monitor.downcast_ref::<gtk4::gdk::Monitor>() {
                    let geometry = monitor.geometry();
                    let monitor_width = geometry.width() as f64;
                    return monitor_width * max_width_percent;
                }
            }
        }

        // Fallback: use a reasonable default
        1920.0 * max_width_percent
    }

    /// Set up the draw handler
    fn setup_draw_handler(&self) {
        let state = self.state.clone();
        let config = self.config.clone();

        self.drawing_area.set_draw_func(move |_area, cr, width, height| {
            draw_minimap(cr, width, height, &state.borrow(), &config.borrow().appearance);
        });
    }
}

/// Calculate total workspace dimensions from windows (excluding floating windows)
fn calculate_workspace_dimensions(state: &MinimapState) -> (f64, f64) {
    let Some(workspace) = state.active_workspace() else {
        return (0.0, 0.0);
    };

    if workspace.windows.is_empty() {
        return (0.0, 0.0);
    }

    // Group windows by column, excluding floating windows
    let mut columns: std::collections::BTreeMap<usize, Vec<&Window>> = std::collections::BTreeMap::new();
    for window in workspace.windows.values() {
        if !window.is_floating {
            columns.entry(window.column_index).or_default().push(window);
        }
    }

    if columns.is_empty() {
        return (0.0, 0.0);
    }

    // Calculate dimensions
    let mut total_width = 0.0f64;
    let mut max_height = 0.0f64;

    for col_idx in 0..=columns.keys().max().copied().unwrap_or(0) {
        if let Some(windows) = columns.get(&col_idx) {
            let col_width = windows.iter().map(|w| w.size.0).fold(0.0f64, f64::max);
            let col_height: f64 = windows.iter().map(|w| w.size.1).sum();
            total_width += col_width;
            max_height = max_height.max(col_height);
        }
    }

    (total_width, max_height)
}

/// Draw the minimap
fn draw_minimap(
    cr: &Context,
    width: i32,
    height: i32,
    state: &MinimapState,
    appearance: &AppearanceConfig,
) {
    let width = width as f64;
    let height = height as f64;

    // Clear with transparency first
    cr.set_operator(Operator::Clear);
    cr.paint().ok();
    cr.set_operator(Operator::Over);

    // Draw background (only if opacity > 0)
    if appearance.background_opacity > 0.0 {
        if let Some(bg_color) = Color::from_hex(&appearance.background) {
            cr.set_source_rgba(bg_color.r, bg_color.g, bg_color.b, appearance.background_opacity);
            rounded_rectangle(cr, 0.0, 0.0, width, height, appearance.border_radius * 2.0);
            cr.fill().ok();
        }
    }

    // Get the active workspace
    let Some(workspace) = state.active_workspace() else {
        return;
    };

    if workspace.windows.is_empty() {
        return;
    }

    // Separate tiled and floating windows
    // Note: floating_windows is not currently implemented for minimap, see comments below.
    let mut tiled_windows: Vec<&Window> = Vec::new();
    let mut floating_windows: Vec<&Window> = Vec::new();

    for window in workspace.windows.values() {
        if window.is_floating {
            floating_windows.push(window);
        } else {
            tiled_windows.push(window);
        }
    }

    // Group tiled windows by column and sort by window_index within each column
    let mut columns: std::collections::BTreeMap<usize, Vec<&Window>> = std::collections::BTreeMap::new();
    for window in tiled_windows {
        columns.entry(window.column_index).or_default().push(window);
    }

    // Sort windows within each column by their window_index
    for windows in columns.values_mut() {
        windows.sort_by_key(|w| w.window_index);
    }

    // Calculate column positions and total dimensions
    let mut column_widths: Vec<f64> = Vec::new();
    let mut column_heights: Vec<f64> = Vec::new();

    for col_idx in 0..=columns.keys().max().copied().unwrap_or(0) {
        if let Some(windows) = columns.get(&col_idx) {
            let max_width = windows.iter().map(|w| w.size.0).fold(0.0f64, f64::max);
            let total_height: f64 = windows.iter().map(|w| w.size.1).sum();
            column_widths.push(max_width);
            column_heights.push(total_height);
        } else {
            column_widths.push(0.0);
            column_heights.push(0.0);
        }
    }

    let total_width: f64 = column_widths.iter().sum();
    let max_height: f64 = column_heights.iter().fold(0.0f64, |a, &b| a.max(b));

    if total_width <= 0.0 || max_height <= 0.0 {
        return;
    }

    // Add padding inside the minimap
    let padding = 4.0;
    let inner_width = width - padding * 2.0;
    let inner_height = height - padding * 2.0;

    // Scale based on height (width is dynamic, so we primarily fit height)
    let scale = inner_height / max_height;

    // Calculate offset - center vertically, left-align horizontally
    let scaled_width = total_width * scale;
    let offset_x = padding + (inner_width - scaled_width).max(0.0) / 2.0;
    let offset_y = padding;

    // Get colors
    let window_color = Color::from_hex(&appearance.window_color)
        .unwrap_or(Color { r: 0.27, g: 0.28, b: 0.35, a: 1.0 });
    let focused_color = Color::from_hex(&appearance.focused_color)
        .unwrap_or(Color { r: 0.54, g: 0.71, b: 0.98, a: 1.0 });
    let border_color = Color::from_hex(&appearance.border_color)
        .unwrap_or(Color { r: 0.42, g: 0.44, b: 0.53, a: 1.0 });

    let gap = appearance.gap;
    let half_gap = gap / 2.0;

    // Calculate x position for each column
    let mut column_x_positions: Vec<f64> = Vec::new();
    let mut x_pos = 0.0;
    for &col_width in &column_widths {
        column_x_positions.push(x_pos);
        x_pos += col_width;
    }

    // Draw each window
    for (&col_idx, windows) in &columns {
        let col_x = column_x_positions.get(col_idx).copied().unwrap_or(0.0);
        let mut y_pos = 0.0;

        for window in windows {
            // Transform coordinates
            let x = offset_x + col_x * scale;
            let y = offset_y + y_pos * scale;
            let w = window.size.0 * scale;
            let h = window.size.1 * scale;

            y_pos += window.size.1;

            // Apply gap (shrink window by half_gap on each side)
            let x = x + half_gap;
            let y = y + half_gap;
            let w = (w - gap).max(1.0);
            let h = (h - gap).max(1.0);

            // Skip windows that are too small to render
            if w < 1.0 || h < 1.0 {
                continue;
            }

            // Choose fill color based on focus state
            let fill_color = if window.is_focused {
                &focused_color
            } else {
                &window_color
            };

            // Draw the window rectangle fill
            cr.set_source_rgba(fill_color.r, fill_color.g, fill_color.b, fill_color.a);
            rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
            cr.fill().ok();

            // Draw border on all windows
            if appearance.border_width > 0.0 {
                cr.set_source_rgba(border_color.r, border_color.g, border_color.b, border_color.a);
                cr.set_line_width(appearance.border_width);
                rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
                cr.stroke().ok();
            }
        }
    }

    // ==================================================================================
    // FLOATING WINDOW RENDERING - CURRENTLY DISABLED
    // ==================================================================================
    // Floating windows are not rendered due to viewport offset tracking limitations.
    // See GitHub Issue #6 for details: https://github.com/alexandergknoll/nirimap/issues/6
    //
    // The code below implements partial floating window support that works when:
    // - A tiled window has focus (can estimate viewport offset from focused column)
    // - Using left-align behavior (center-focused-column "never" in Niri config)
    //
    // Known issues that prevent enabling this:
    // 1. When a floating window has focus, viewport offset cannot be determined
    // 2. Viewport can scroll without focus changes (center-column, vertical stacking)
    // 3. Assumes specific Niri configuration (center-focused-column "never")
    //
    // This code is preserved for future use when Niri's IPC exposes viewport position.
    // ==================================================================================

    /* DISABLED - Floating window rendering

    // Estimate viewport offset based on focused column (assumes left-align behavior)
    let viewport_offset = if !columns.is_empty() {
        // Find which column is focused (tiled windows only)
        let focused_col = workspace.windows.values()
            .find(|w| w.is_focused && !w.is_floating)
            .map(|w| w.column_index);

        if let Some(focused_col_idx) = focused_col {
            // With left-align (center-focused-column "never"), the focused column
            // is positioned at the left edge of the viewport
            // So viewport_offset = x position of the focused column
            let offset = column_x_positions.get(focused_col_idx).copied().unwrap_or(0.0);

            tracing::debug!("Focused column: {}, Viewport offset (left-align): {}", focused_col_idx, offset);

            offset
        } else {
            // No tiled window focused (floating window is focused)
            // We can't determine viewport offset, so assume it hasn't changed
            // This is a limitation - we'd need to track the last known offset
            tracing::debug!("No tiled window focused (floating focused), using offset: 0");
            0.0
        }
    } else {
        0.0
    };

    for window in floating_windows {
        // Floating windows in Niri are already viewport-relative (like tiled windows)
        // Their position is screen-relative, so we need to ADD the viewport offset
        // to convert to workspace coordinates for rendering on the minimap
        let workspace_x = window.pos.0 + viewport_offset;
        let workspace_y = window.pos.1;

        tracing::debug!("Drawing floating window {}: viewport_pos=({}, {}), viewport_offset={}, workspace_pos=({}, {})",
            window.id, window.pos.0, window.pos.1, viewport_offset, workspace_x, workspace_y);

        // Transform to minimap coordinates
        let x = offset_x + workspace_x * scale;
        let y = offset_y + workspace_y * scale;
        let w = window.size.0 * scale;
        let h = window.size.1 * scale;

        // Apply gap
        let x = x + half_gap;
        let y = y + half_gap;
        let w = (w - gap).max(1.0);
        let h = (h - gap).max(1.0);

        // Skip windows that are too small to render
        if w < 1.0 || h < 1.0 {
            continue;
        }

        // Choose fill color based on focus state
        let fill_color = if window.is_focused {
            &focused_color
        } else {
            &window_color
        };

        // Draw the window rectangle fill
        cr.set_source_rgba(fill_color.r, fill_color.g, fill_color.b, fill_color.a);
        rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
        cr.fill().ok();

        // Draw border
        if appearance.border_width > 0.0 {
            cr.set_source_rgba(border_color.r, border_color.g, border_color.b, border_color.a);
            cr.set_line_width(appearance.border_width);
            rounded_rectangle(cr, x, y, w, h, appearance.border_radius);
            cr.stroke().ok();
        }
    }

    */ // END DISABLED floating window rendering
}

/// Draw a rounded rectangle path
fn rounded_rectangle(cr: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);

    cr.new_path();
    cr.arc(x + width - radius, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + width - radius, y + height - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + radius, y + height - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + radius, y + radius, radius, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}
