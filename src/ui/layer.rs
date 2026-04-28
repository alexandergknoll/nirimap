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

    // Tag the window with a CSS class so our transparency rules can target it
    // with high specificity (themes commonly set `.background { ... !important }`,
    // which beats `* { ... !important }` due to specificity).
    window.add_css_class("nirimap-window");

    // Initialize layer shell
    window.init_layer_shell();

    // Set the namespace for layer rules
    window.set_namespace(Some("nirimap"));

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

    // Set up CSS for transparency. GTK renders widget CSS backgrounds in a
    // separate render node beneath our Cairo content, so we must zero it out
    // via CSS. Use high-specificity selectors targeting our own CSS class so
    // theme rules (often `.background { ... !important }`, specificity 0,0,1,0)
    // can't beat us. Combine class + tag for specificity 0,0,1,1.
    let css_provider = gtk4::CssProvider::new();
    css_provider.connect_parsing_error(|_, section, error| {
        tracing::error!(
            "nirimap transparency CSS parse error at {:?}: {}",
            section,
            error
        );
    });
    css_provider.load_from_data(
        "window.nirimap-window,
         window.nirimap-window.background,
         window.nirimap-window > widget,
         window.nirimap-window > drawingarea,
         drawingarea.nirimap-canvas {
             background-color: transparent;
             background-image: none;
             box-shadow: none;
         }",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not get default display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_USER,
    );

    // Set up empty input region for true click-through at Wayland level
    window.connect_realize(|window| {
        if let Some(surface) = window.surface() {
            // Create an empty region for input - this makes the surface click-through
            let empty_region = gtk4::cairo::Region::create();
            surface.set_input_region(Some(&empty_region));
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
