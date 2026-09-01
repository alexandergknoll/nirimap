use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Minimap placement on screen, and label/icon placement within rectangle
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    TopCenter,
    #[default]
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Center,
}

/// Which workspaces the minimap renders
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceMode {
    /// Render only the currently active workspace
    Current,
    /// Render every workspace stacked vertically (Overview-style)
    #[default]
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Per-workspace row height in pixels. In `current` mode this is the whole
    /// widget height; in `all` mode it is the height of a single workspace row.
    pub height: u32,
    /// Maximum width as percentage of screen width (0.0 - 1.0)
    pub max_width_percent: f64,
    /// Maximum height as percentage of screen height (0.0 - 1.0), used in `all` mode
    pub max_height_percent: f64,
    pub anchor: Anchor,
    /// Horizontal margin from edge
    pub margin_x: i32,
    /// Vertical margin from edge
    pub margin_y: i32,
    pub workspace_mode: WorkspaceMode,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            height: 100,
            max_width_percent: 0.5,
            max_height_percent: 0.8,
            anchor: Anchor::TopRight,
            margin_x: 10,
            margin_y: 10,
            workspace_mode: WorkspaceMode::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    /// Background color (hex)
    pub background: String,
    /// Default window rectangle color (hex)
    pub window_color: String,
    /// Focused window highlight color (hex)
    pub focused_color: String,
    /// Window border color (hex)
    pub border_color: String,
    /// Window border thickness
    pub border_width: f64,
    /// Corner radius for window rectangles
    pub border_radius: f64,
    /// Gap between windows (in minimap pixels)
    pub gap: f64,
    /// Background opacity (0.0 = transparent, 1.0 = opaque)
    pub background_opacity: f64,
    /// Fill opacity for unfocused windows (0.0 = transparent, just borders)
    pub window_opacity: f64,
    /// Fill opacity for the focused window
    pub focused_opacity: f64,
    /// Vertical gap between stacked workspaces in `all` mode
    pub workspace_gap: f64,
    /// Border color for the active workspace in `all` mode (hex)
    pub active_workspace_border_color: String,
    /// Border thickness for the active workspace in `all` mode
    pub active_workspace_border_width: f64,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            background: "#1e1e2e".to_string(),
            window_color: "#45475a".to_string(),
            focused_color: "#89b4fa".to_string(),
            border_color: "#6c7086".to_string(),
            border_width: 1.0,
            border_radius: 2.0,
            gap: 2.0,
            background_opacity: 0.0,
            window_opacity: 0.7,
            focused_opacity: 1.0,
            workspace_gap: 4.0,
            active_workspace_border_color: "#89b4fa".to_string(),
            active_workspace_border_width: 2.0,
        }
    }
}

/// What text a window label shows
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LabelContent {
    /// Window title
    #[default]
    Title,
    /// Application ID
    AppId,
    /// Application ID and title, e.g. "firefox — GitHub"
    AppIdTitle,
    /// No text (labels effectively disabled)
    None,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
}

/// Icon size: fixed pixel size or scaled from the window rectangle
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum IconSize {
    /// Scale with the window rectangle
    #[default]
    Auto,
    /// Explicit pixel size
    Pixels(f64),
}

impl<'de> serde::Deserialize<'de> for IconSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Number(f64),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Text(s) if s == "auto" => Ok(IconSize::Auto),
            Raw::Text(s) => Err(serde::de::Error::custom(format!(
                "invalid icon size \"{}\": expected \"auto\" or a number",
                s
            ))),
            Raw::Number(n) => Ok(IconSize::Pixels(n)),
        }
    }
}

/// Window label configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LabelConfig {
    /// Draw text labels on window rectangles
    pub enabled: bool,
    /// What the label shows
    pub content: LabelContent,
    pub font_family: String,
    /// Font size in pixels
    pub font_size: f64,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    /// Text color (hex)
    pub color: String,
    /// Text color on the focused window (hex)
    pub focused_color: String,
    /// Anchor position within the window rectangle
    pub position: Anchor,
    /// Inner padding between label and window edge
    pub padding: f64,
    /// Skip labels on windows whose rectangle is smaller than this (minimap pixels)
    pub min_window_size: f64,
    /// Draw a dark drop shadow behind the text for legibility
    pub shadow: bool,
}

impl Default for LabelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            content: LabelContent::Title,
            font_family: "Sans".to_string(),
            font_size: 10.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            color: "#cdd6f4".to_string(),
            focused_color: "#1e1e2e".to_string(),
            position: Anchor::Center,
            padding: 2.0,
            min_window_size: 30.0,
            shadow: false,
        }
    }
}

/// Application icon configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IconConfig {
    /// Draw application icons on window rectangles
    pub enabled: bool,
    /// Icon size: "auto" (scales with the window rectangle) or explicit pixels
    pub size: IconSize,
    /// Anchor position within the window rectangle
    pub position: Anchor,
    /// Icon opacity (0.0 - 1.0)
    pub opacity: f64,
    /// GTK icon theme name to use instead of the system default
    pub theme_override: Option<String>,
    /// Skip icons on windows whose rectangle is smaller than this (minimap pixels)
    pub min_window_size: f64,
}

impl Default for IconConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size: IconSize::Auto,
            position: Anchor::Center,
            opacity: 1.0,
            theme_override: None,
            min_window_size: 16.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Keep visible in Niri overview mode
    pub show_on_overview: bool,
    /// Always show the minimap (if false, only shows on focus change)
    pub always_visible: bool,
    /// How long to stay visible after a show, when `always_visible` is false
    pub hide_timeout_ms: u32,
    /// Whether floating-window focus/spawn events show the minimap (when not
    /// `always_visible`). Floating windows aren't drawn, so this is off by default.
    pub show_for_floating_windows: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            show_on_overview: true,
            always_visible: true,
            hide_timeout_ms: 2000,
            show_for_floating_windows: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub display: DisplayConfig,
    pub appearance: AppearanceConfig,
    pub labels: LabelConfig,
    pub icons: IconConfig,
    pub behavior: BehaviorConfig,
}

impl Config {
    /// Load the config file, writing the default one first if it doesn't exist.
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path).with_context(|| {
                format!("Failed to read config file: {}", config_path.display())
            })?;

            let config: Config = toml::from_str(&contents).with_context(|| {
                format!("Failed to parse config file: {}", config_path.display())
            })?;

            Ok(config)
        } else {
            let config = Config::default();
            config.save_default()?;
            Ok(config)
        }
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .expect("Failed to determine config directory. Please set XDG_CONFIG_HOME or HOME environment variable.")
            .join("nirimap")
            .join("config.toml")
    }

    fn save_default(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let default_config = DEFAULT_CONFIG_TOML;
        std::fs::write(&config_path, default_config).with_context(|| {
            format!("Failed to write default config: {}", config_path.display())
        })?;

        tracing::info!("Created default config at {}", config_path.display());
        Ok(())
    }
}

/// The default config file contents, written on first run.
const DEFAULT_CONFIG_TOML: &str = r##"[display]
height = 100              # Per-workspace row height in pixels
                          # In "current" mode: total widget height
                          # In "all" mode: height of one workspace row
max_width_percent = 0.5   # Maximum width as fraction of screen (0.0 - 1.0)
max_height_percent = 0.8  # Maximum height as fraction of screen (used in "all" mode)
anchor = "top-right"      # Position: top-left, top-center, top-right,
                          #           bottom-left, bottom-center, bottom-right, center
margin_x = 10             # Horizontal margin from edge
margin_y = 10             # Vertical margin from edge
workspace_mode = "all"    # Which workspaces to show:
                          #   "all"     - stack every workspace vertically (Overview-style)
                          #   "current" - show only the active workspace

[appearance]
background = "#1e1e2e"    # Background color (hex)
window_color = "#45475a"  # Default window rectangle color
focused_color = "#89b4fa" # Focused window highlight
border_color = "#6c7086"  # Window border color
border_width = 1          # Window border thickness
border_radius = 2         # Corner radius for window rectangles
gap = 2                   # Gap between windows (in minimap pixels)
background_opacity = 0.0  # Background opacity (0.0 = transparent, 1.0 = opaque)
                          # Applies in both "current" and "all" modes
window_opacity = 0.7      # Fill opacity for unfocused windows (0 = outlines only)
focused_opacity = 1.0     # Fill opacity for the focused window
workspace_gap = 4                            # Vertical gap between stacked workspaces ("all" mode)
active_workspace_border_color = "#89b4fa"    # Highlight border for active workspace ("all" mode)
active_workspace_border_width = 2            # Highlight border thickness ("all" mode)

[labels]
enabled = false           # Draw text labels on window rectangles
content = "title"         # What to show: "title", "app-id", "app-id-title", "none"
font_family = "Sans"      # Font family name
font_size = 10            # Font size in pixels
font_weight = "normal"    # "normal" or "bold"
font_style = "normal"     # "normal" or "italic"
color = "#cdd6f4"         # Text color
focused_color = "#1e1e2e" # Text color on the focused window
position = "center"       # Anchor within the window rectangle: center, top-left,
                          # top-center, top-right, bottom-left, bottom-center, bottom-right
padding = 2               # Inner padding between label and window edge
min_window_size = 30      # Skip labels on rectangles smaller than this (minimap pixels)
shadow = false            # Dark drop shadow behind text for legibility

[icons]
enabled = true            # Draw application icons on window rectangles
size = "auto"             # "auto" (scales with the rectangle) or explicit pixels, e.g. 16
position = "center"       # Anchor within the window rectangle (same options as labels)
opacity = 1.0             # Icon opacity (0.0 - 1.0)
# theme_override = "Papirus" # GTK icon theme name (defaults to system theme)
min_window_size = 16      # Skip icons on rectangles smaller than this (minimap pixels)

[behavior]
show_on_overview = true        # Keep visible in Niri overview mode
always_visible = true          # Always show minimap (false = only on focus change)
hide_timeout_ms = 2000         # Milliseconds before hiding after focus change
show_for_floating_windows = false # When always_visible = false, surface the minimap for
                                  # floating-window events (focus to/from a floating window,
                                  # floating window spawn). Off by default since floating
                                  # windows aren't drawn on the minimap.
"##;

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    /// Parse `#rrggbb` (the `#` is optional).
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');

        if hex.len() != 6 {
            return None;
        }

        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

        Some(Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
            a: 1.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = Config::default();

        assert_eq!(config.display.height, 100);
        assert_eq!(config.display.max_width_percent, 0.5);
        assert_eq!(config.display.max_height_percent, 0.8);
        assert_eq!(config.display.anchor, Anchor::TopRight);
        assert_eq!(config.display.margin_x, 10);
        assert_eq!(config.display.margin_y, 10);
        assert_eq!(config.display.workspace_mode, WorkspaceMode::All);

        assert_eq!(config.appearance.background, "#1e1e2e");
        assert_eq!(config.appearance.window_color, "#45475a");
        assert_eq!(config.appearance.focused_color, "#89b4fa");
        assert_eq!(config.appearance.border_color, "#6c7086");
        assert_eq!(config.appearance.border_width, 1.0);
        assert_eq!(config.appearance.border_radius, 2.0);
        assert_eq!(config.appearance.gap, 2.0);
        assert_eq!(config.appearance.background_opacity, 0.0);
        assert_eq!(config.appearance.window_opacity, 0.7);
        assert_eq!(config.appearance.focused_opacity, 1.0);
        assert_eq!(config.appearance.workspace_gap, 4.0);
        assert_eq!(config.appearance.active_workspace_border_color, "#89b4fa");
        assert_eq!(config.appearance.active_workspace_border_width, 2.0);

        assert!(!config.labels.enabled);
        assert_eq!(config.labels.content, LabelContent::Title);
        assert_eq!(config.labels.font_family, "Sans");
        assert_eq!(config.labels.font_size, 10.0);
        assert_eq!(config.labels.font_weight, FontWeight::Normal);
        assert_eq!(config.labels.font_style, FontStyle::Normal);
        assert_eq!(config.labels.color, "#cdd6f4");
        assert_eq!(config.labels.focused_color, "#1e1e2e");
        assert_eq!(config.labels.position, Anchor::Center);
        assert_eq!(config.labels.padding, 2.0);
        assert_eq!(config.labels.min_window_size, 30.0);
        assert!(!config.labels.shadow);

        assert!(config.icons.enabled);
        assert_eq!(config.icons.size, IconSize::Auto);
        assert_eq!(config.icons.position, Anchor::Center);
        assert_eq!(config.icons.opacity, 1.0);
        assert_eq!(config.icons.theme_override, None);
        assert_eq!(config.icons.min_window_size, 16.0);

        assert!(config.behavior.show_on_overview);
        assert!(config.behavior.always_visible);
        assert_eq!(config.behavior.hide_timeout_ms, 2000);
        assert!(!config.behavior.show_for_floating_windows);
    }

    #[test]
    fn test_label_config_deserialization() {
        let toml = r#"
            [labels]
            enabled = true
            content = "app-id-title"
            font_weight = "bold"
            font_style = "italic"
            position = "top-left"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.labels.enabled);
        assert_eq!(config.labels.content, LabelContent::AppIdTitle);
        assert_eq!(config.labels.font_weight, FontWeight::Bold);
        assert_eq!(config.labels.font_style, FontStyle::Italic);
        assert_eq!(config.labels.position, Anchor::TopLeft);
        // Unspecified fields keep defaults
        assert_eq!(config.labels.font_family, "Sans");
        assert_eq!(config.labels.padding, 2.0);
    }

    #[test]
    fn test_label_content_variants() {
        for (value, expected) in [
            ("title", LabelContent::Title),
            ("app-id", LabelContent::AppId),
            ("app-id-title", LabelContent::AppIdTitle),
            ("none", LabelContent::None),
        ] {
            let toml = format!("[labels]\ncontent = \"{}\"", value);
            let config: Config = toml::from_str(&toml).unwrap();
            assert_eq!(config.labels.content, expected);
        }
    }

    #[test]
    fn test_icon_config_deserialization() {
        let toml = r#"
            [icons]
            enabled = false
            size = 24
            position = "bottom-right"
            opacity = 0.5
            theme_override = "Papirus"
            min_window_size = 10
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.icons.enabled);
        assert_eq!(config.icons.size, IconSize::Pixels(24.0));
        assert_eq!(config.icons.position, Anchor::BottomRight);
        assert_eq!(config.icons.opacity, 0.5);
        assert_eq!(config.icons.theme_override.as_deref(), Some("Papirus"));
        assert_eq!(config.icons.min_window_size, 10.0);
    }

    #[test]
    fn test_icon_size_auto_deserialization() {
        let toml = r#"
            [icons]
            size = "auto"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.icons.size, IconSize::Auto);
    }

    #[test]
    fn test_icon_size_invalid_string_rejected() {
        let toml = r#"
            [icons]
            size = "huge"
        "#;
        assert!(toml::from_str::<Config>(toml).is_err());
    }

    #[test]
    fn test_default_config_toml_matches_defaults() {
        // The file written on first run must agree with the in-code defaults.
        let config: Config = toml::from_str(DEFAULT_CONFIG_TOML).unwrap();
        let defaults = Config::default();

        assert_eq!(config.display.height, defaults.display.height);
        assert_eq!(config.appearance.background, defaults.appearance.background);
        assert_eq!(config.labels.enabled, defaults.labels.enabled);
        assert_eq!(config.labels.content, defaults.labels.content);
        assert_eq!(config.labels.font_size, defaults.labels.font_size);
        assert_eq!(
            config.labels.min_window_size,
            defaults.labels.min_window_size
        );
        assert_eq!(config.icons.enabled, defaults.icons.enabled);
        assert_eq!(config.icons.size, defaults.icons.size);
        assert_eq!(config.icons.opacity, defaults.icons.opacity);
        assert_eq!(config.icons.theme_override, defaults.icons.theme_override);
        assert_eq!(
            config.behavior.always_visible,
            defaults.behavior.always_visible
        );
    }

    #[test]
    fn test_anchor_deserialization() {
        let toml = r#"
            [display]
            anchor = "top-left"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.anchor, Anchor::TopLeft);

        let toml = r#"
            [display]
            anchor = "bottom-center"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.anchor, Anchor::BottomCenter);
    }

    #[test]
    fn test_workspace_mode_deserialization() {
        let toml = r#"
            [display]
            workspace_mode = "current"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.workspace_mode, WorkspaceMode::Current);

        let toml = r#"
            [display]
            workspace_mode = "all"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.workspace_mode, WorkspaceMode::All);

        let config = Config::default();
        assert_eq!(config.display.workspace_mode, WorkspaceMode::All);
    }

    #[test]
    fn test_partial_config_override() {
        let toml = r#"
            [display]
            height = 150
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.height, 150);
        assert_eq!(config.display.max_width_percent, 0.5);
        assert_eq!(config.appearance.background, "#1e1e2e");
    }

    #[test]
    fn test_color_from_hex_with_hash() {
        let color = Color::from_hex("#1e1e2e").unwrap();
        assert!((color.r - 30.0 / 255.0).abs() < 0.001);
        assert!((color.g - 30.0 / 255.0).abs() < 0.001);
        assert!((color.b - 46.0 / 255.0).abs() < 0.001);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_color_from_hex_without_hash() {
        let color = Color::from_hex("89b4fa").unwrap();
        assert!((color.r - 137.0 / 255.0).abs() < 0.001);
        assert!((color.g - 180.0 / 255.0).abs() < 0.001);
        assert!((color.b - 250.0 / 255.0).abs() < 0.001);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_color_from_hex_invalid_length() {
        assert!(Color::from_hex("#fff").is_none());
        assert!(Color::from_hex("#1e1e2e00").is_none());
        assert!(Color::from_hex("").is_none());
    }

    #[test]
    fn test_color_from_hex_invalid_characters() {
        assert!(Color::from_hex("#gggggg").is_none());
        assert!(Color::from_hex("#1e1e2z").is_none());
        assert!(Color::from_hex("xyz123").is_none());
    }

    #[test]
    fn test_color_from_hex_edge_cases() {
        let black = Color::from_hex("#000000").unwrap();
        assert_eq!(black.r, 0.0);
        assert_eq!(black.g, 0.0);
        assert_eq!(black.b, 0.0);

        let white = Color::from_hex("#ffffff").unwrap();
        assert_eq!(white.r, 1.0);
        assert_eq!(white.g, 1.0);
        assert_eq!(white.b, 1.0);
    }

    #[test]
    fn test_color_alpha_is_always_one() {
        let color1 = Color::from_hex("#123456").unwrap();
        assert_eq!(color1.a, 1.0);

        let color2 = Color::from_hex("abcdef").unwrap();
        assert_eq!(color2.a, 1.0);
    }
}
