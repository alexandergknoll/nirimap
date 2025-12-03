use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{Anchor, Config};

/// Create and configure a layer-shell window for the minimap
pub fn create_layer_window(app: &Application, config: &Config) -> ApplicationWindow {
    // Start with height from config; width will be set dynamically
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(config.display.height as i32) // Start square, will resize
        .default_height(config.display.height as i32)
        .decorated(false)
        .resizable(true) // Allow resizing for dynamic width
        .build();

    // Initialize layer shell
    window.init_layer_shell();

    // Set the namespace for layer rules
    window.set_namespace("nirimap");

    // Set layer to overlay (above fullscreen windows)
    window.set_layer(Layer::Overlay);

    // Don't reserve exclusive screen space
    window.set_exclusive_zone(0);

    // No keyboard interactivity (read-only minimap)
    window.set_keyboard_mode(KeyboardMode::None);

    // Make window click-through (don't receive pointer events at GTK level)
    window.set_can_target(false);

    // Configure anchor based on config
    configure_anchor(&window, config);

    // Set margins
    window.set_margin(Edge::Top, config.display.margin_y);
    window.set_margin(Edge::Bottom, config.display.margin_y);
    window.set_margin(Edge::Left, config.display.margin_x);
    window.set_margin(Edge::Right, config.display.margin_x);

    // Set up CSS for transparency support
    let css_provider = gtk4::CssProvider::new();
    css_provider.load_from_data("window { background: transparent; }");
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Set up empty input region for true click-through at Wayland level
    window.connect_realize(|window| {
        if let Some(surface) = window.surface() {
            // Create an empty region for input - this makes the surface click-through
            let empty_region = gtk4::cairo::Region::create();
            surface.set_input_region(&empty_region);
        }
    });

    window
}

/// Configure the window anchor position based on config
fn configure_anchor(window: &ApplicationWindow, config: &Config) {
    // First, unset all anchors
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Bottom, false);
    window.set_anchor(Edge::Left, false);
    window.set_anchor(Edge::Right, false);

    // Set appropriate anchors based on config
    match config.display.anchor {
        Anchor::TopLeft => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
        }
        Anchor::TopCenter => {
            window.set_anchor(Edge::Top, true);
            // No left/right anchor = centered horizontally
        }
        Anchor::TopRight => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Right, true);
        }
        Anchor::BottomLeft => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
        }
        Anchor::BottomCenter => {
            window.set_anchor(Edge::Bottom, true);
            // No left/right anchor = centered horizontally
        }
        Anchor::BottomRight => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Right, true);
        }
        Anchor::Center => {
            // No anchors = centered both horizontally and vertically
        }
    }
}
