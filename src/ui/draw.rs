//! Resources shared by everything drawn in one frame.

use gtk4::cairo::Context;

use super::decorations::IconCache;
use crate::config::{Color, Config};

pub(super) struct DrawCtx<'a> {
    pub cr: &'a Context,
    pub config: &'a Config,
    pub palette: Palette,
    pub icon_cache: &'a mut IconCache,
    /// Display scale factor, so icons are rasterized at device resolution.
    pub widget_scale: i32,
}

/// The config's hex colours, parsed once per config load rather than per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Palette {
    /// `None` if the configured value is invalid: no background is drawn.
    pub background: Option<Color>,
    pub window: Color,
    pub focused: Color,
    pub border: Color,
    pub active_workspace_border: Color,
    pub label: Color,
    pub label_focused: Color,
}

// Used in place of invalid configured colours.
const FALLBACK_WINDOW: Color = Color::rgb(0.27, 0.28, 0.35);
const FALLBACK_FOCUSED: Color = Color::rgb(0.54, 0.71, 0.98);
const FALLBACK_BORDER: Color = Color::rgb(0.42, 0.44, 0.53);
const FALLBACK_LABEL: Color = Color::rgb(0.8, 0.84, 0.96);

impl Palette {
    /// Parse every colour in `config`, warning about invalid values.
    pub fn from_config(config: &Config) -> Self {
        let appearance = &config.appearance;
        let labels = &config.labels;

        Self {
            background: parse("appearance.background", &appearance.background),
            window: parse("appearance.window_color", &appearance.window_color)
                .unwrap_or(FALLBACK_WINDOW),
            focused: parse("appearance.focused_color", &appearance.focused_color)
                .unwrap_or(FALLBACK_FOCUSED),
            border: parse("appearance.border_color", &appearance.border_color)
                .unwrap_or(FALLBACK_BORDER),
            active_workspace_border: parse(
                "appearance.active_workspace_border_color",
                &appearance.active_workspace_border_color,
            )
            .unwrap_or(FALLBACK_FOCUSED),
            label: parse("labels.color", &labels.color).unwrap_or(FALLBACK_LABEL),
            label_focused: parse("labels.focused_color", &labels.focused_color)
                .unwrap_or(FALLBACK_LABEL),
        }
    }
}

fn parse(key: &str, hex: &str) -> Option<Color> {
    let color = Color::from_hex(hex);
    if color.is_none() {
        tracing::warn!(
            "Invalid colour {:?} for {}: expected \"#rrggbb\"; using a fallback",
            hex,
            key
        );
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_parses_default_config() {
        let palette = Palette::from_config(&Config::default());

        assert_eq!(palette.background, Color::from_hex("#1e1e2e"));
        assert_eq!(palette.window, Color::from_hex("#45475a").unwrap());
        assert_eq!(palette.focused, Color::from_hex("#89b4fa").unwrap());
        assert_eq!(palette.border, Color::from_hex("#6c7086").unwrap());
        assert_eq!(
            palette.active_workspace_border,
            Color::from_hex("#89b4fa").unwrap()
        );
        assert_eq!(palette.label, Color::from_hex("#cdd6f4").unwrap());
        assert_eq!(palette.label_focused, Color::from_hex("#1e1e2e").unwrap());
    }

    #[test]
    fn test_palette_falls_back_on_invalid_colours() {
        let mut config = Config::default();
        config.appearance.background = "nope".to_string();
        config.appearance.window_color = "#fff".to_string();
        config.appearance.focused_color = String::new();
        config.appearance.border_color = "#gggggg".to_string();
        config.appearance.active_workspace_border_color = "#1234567".to_string();
        config.labels.color = "red".to_string();
        config.labels.focused_color = "#12".to_string();

        let palette = Palette::from_config(&config);

        assert_eq!(palette.background, None);
        assert_eq!(palette.window, FALLBACK_WINDOW);
        assert_eq!(palette.focused, FALLBACK_FOCUSED);
        assert_eq!(palette.border, FALLBACK_BORDER);
        assert_eq!(palette.active_workspace_border, FALLBACK_FOCUSED);
        assert_eq!(palette.label, FALLBACK_LABEL);
        assert_eq!(palette.label_focused, FALLBACK_LABEL);
    }
}
