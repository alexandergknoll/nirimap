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

/// Display configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Minimap height in pixels (width is calculated dynamically)
    pub height: u32,
    /// Maximum width as percentage of screen width (0.0 - 1.0)
    pub max_width_percent: f64,
    /// Position anchor
    pub anchor: Anchor,
    /// Horizontal margin from edge
    pub margin_x: i32,
    /// Vertical margin from edge
    pub margin_y: i32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            height: 100,
            max_width_percent: 0.5,
            anchor: Anchor::TopRight,
            margin_x: 10,
            margin_y: 10,
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
            background_opacity: 0.9,
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
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            show_on_overview: true,
            always_visible: true,
            hide_timeout_ms: 2000,
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
            let contents = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

            let config: Config = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

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
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }

        let default_config = r##"[display]
height = 100              # Minimap height in pixels (width is dynamic)
max_width_percent = 0.5   # Maximum width as fraction of screen (0.0 - 1.0)
anchor = "top-right"      # Position: top-left, top-center, top-right,
                          #           bottom-left, bottom-center, bottom-right, center
margin_x = 10             # Horizontal margin from edge
margin_y = 10             # Vertical margin from edge

[appearance]
background = "#1e1e2e"    # Background color (hex)
window_color = "#45475a"  # Default window rectangle color
focused_color = "#89b4fa" # Focused window highlight
border_color = "#6c7086"  # Window border color
border_width = 1          # Window border thickness
border_radius = 2         # Corner radius for window rectangles
gap = 2                   # Gap between windows (in minimap pixels)
background_opacity = 0.9  # Background opacity (0.0 = transparent, 1.0 = opaque)

[behavior]
show_on_overview = true   # Keep visible in Niri overview mode
always_visible = true     # Always show minimap (false = only on focus change)
hide_timeout_ms = 2000    # Milliseconds before hiding after focus change
"##;

        std::fs::write(&config_path, default_config)
            .with_context(|| format!("Failed to write default config: {}", config_path.display()))?;

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
