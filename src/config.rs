use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Anchor position for the minimap on screen
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

/// Display configuration
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
    /// Position anchor
    pub anchor: Anchor,
    /// Horizontal margin from edge
    pub margin_x: i32,
    /// Vertical margin from edge
    pub margin_y: i32,
    /// Which workspaces to display
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

/// Appearance configuration
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

/// Behavior configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Keep visible in Niri overview mode
    pub show_on_overview: bool,
    /// Always show the minimap (if false, only shows on focus change)
    pub always_visible: bool,
    /// Milliseconds to keep minimap visible after focus change (only when always_visible is false)
    pub hide_timeout_ms: u32,
    /// Whether floating-window events (focus, spawn) trigger the minimap to
    /// show (only when `always_visible` is false). Floating windows aren't
    /// rendered on the minimap, so surfacing it for transient popups, dialogs,
    /// or returning focus from a popup is rarely useful.
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

/// Main configuration struct
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub display: DisplayConfig,
    pub appearance: AppearanceConfig,
    pub behavior: BehaviorConfig,
}

impl Config {
    /// Load configuration from the default path or create default config
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
            // Create default config file
            let config = Config::default();
            config.save_default()?;
            Ok(config)
        }
    }

    /// Get the configuration file path
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .expect("Failed to determine config directory. Please set XDG_CONFIG_HOME or HOME environment variable.")
            .join("nirimap")
            .join("config.toml")
    }

    /// Save default configuration to disk
    fn save_default(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let default_config = r##"[display]
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

[behavior]
show_on_overview = true        # Keep visible in Niri overview mode
always_visible = true          # Always show minimap (false = only on focus change)
hide_timeout_ms = 2000         # Milliseconds before hiding after focus change
show_for_floating_windows = false # When always_visible = false, surface the minimap for
                                  # floating-window events (focus to/from a floating window,
                                  # floating window spawn). Off by default since floating
                                  # windows aren't drawn on the minimap.
"##;

        std::fs::write(&config_path, default_config).with_context(|| {
            format!("Failed to write default config: {}", config_path.display())
        })?;

        tracing::info!("Created default config at {}", config_path.display());
        Ok(())
    }
}

/// RGBA color representation
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    /// Parse a hex color string (e.g., "#1e1e2e" or "1e1e2e")
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

        // Test display defaults
        assert_eq!(config.display.height, 100);
        assert_eq!(config.display.max_width_percent, 0.5);
        assert_eq!(config.display.max_height_percent, 0.8);
        assert_eq!(config.display.anchor, Anchor::TopRight);
        assert_eq!(config.display.margin_x, 10);
        assert_eq!(config.display.margin_y, 10);
        assert_eq!(config.display.workspace_mode, WorkspaceMode::All);

        // Test appearance defaults
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

        // Test behavior defaults
        assert!(config.behavior.show_on_overview);
        assert!(config.behavior.always_visible);
        assert_eq!(config.behavior.hide_timeout_ms, 2000);
        assert!(!config.behavior.show_for_floating_windows);
    }

    #[test]
    fn test_anchor_deserialization() {
        // Test that anchor positions are correctly deserialized from TOML
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

        // Default should be All
        let config = Config::default();
        assert_eq!(config.display.workspace_mode, WorkspaceMode::All);
    }

    #[test]
    fn test_partial_config_override() {
        // Test that partial config can be deserialized (uses defaults for missing fields)
        let toml = r#"
            [display]
            height = 150
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.display.height, 150);
        // Other fields should use defaults
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
        // Too short
        assert!(Color::from_hex("#fff").is_none());
        // Too long
        assert!(Color::from_hex("#1e1e2e00").is_none());
        // Empty
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
        // Black
        let black = Color::from_hex("#000000").unwrap();
        assert_eq!(black.r, 0.0);
        assert_eq!(black.g, 0.0);
        assert_eq!(black.b, 0.0);

        // White
        let white = Color::from_hex("#ffffff").unwrap();
        assert_eq!(white.r, 1.0);
        assert_eq!(white.g, 1.0);
        assert_eq!(white.b, 1.0);
    }

    #[test]
    fn test_color_alpha_is_always_one() {
        // Verify that alpha is always 1.0 regardless of input
        let color1 = Color::from_hex("#123456").unwrap();
        assert_eq!(color1.a, 1.0);

        let color2 = Color::from_hex("abcdef").unwrap();
        assert_eq!(color2.a, 1.0);
    }
}
