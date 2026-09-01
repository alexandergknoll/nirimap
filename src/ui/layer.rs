use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{Anchor, Config};

/// Create the layer-shell overlay window.
pub fn create_layer_window(app: &Application, config: &Config) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(config.display.height as i32) // square until content sizes it
        .default_height(config.display.height as i32)
        .decorated(false)
        .resizable(true)
        .build();

    // Targeted by the transparency CSS below.
    window.add_css_class("nirimap-window");

    window.init_layer_shell();
    window.set_namespace(Some("nirimap"));
    window.set_layer(Layer::Overlay); // above fullscreen windows
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::None);
    // GTK-level click-through; the Wayland-level input region is emptied on realize.
    window.set_can_target(false);

    configure_anchor(&window, config);
    window.set_margin(Edge::Top, config.display.margin_y);
    window.set_margin(Edge::Bottom, config.display.margin_y);
    window.set_margin(Edge::Left, config.display.margin_x);
    window.set_margin(Edge::Right, config.display.margin_x);

    // GTK paints widget CSS backgrounds beneath our Cairo content, so zero them
    // out. Element + class selectors (specificity 0,1,1) beat theme rules like
    // `.background { ... !important }`.
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

    // Empty input region: click-through at the Wayland level.
    window.connect_realize(|window| {
        if let Some(surface) = window.surface() {
            let empty_region = gtk4::cairo::Region::create();
            surface.set_input_region(Some(&empty_region));
        }
    });

    window
}

fn configure_anchor(window: &ApplicationWindow, config: &Config) {
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Bottom, false);
    window.set_anchor(Edge::Left, false);
    window.set_anchor(Edge::Right, false);

    match config.display.anchor {
        Anchor::TopLeft => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
        }
        Anchor::TopCenter => {
            window.set_anchor(Edge::Top, true); // unanchored horizontally = centered
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
            window.set_anchor(Edge::Bottom, true); // unanchored horizontally = centered
        }
        Anchor::BottomRight => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Right, true);
        }
        Anchor::Center => {} // unanchored = centered both ways
    }
}
