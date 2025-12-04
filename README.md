# nirimap

A minimal workspace minimap overlay for the [Niri](https://github.com/YaLTeR/niri) Wayland compositor.

> [!WARNING]
> **AI-Generated Code**: This application was created by Claude Code. Please review the source code before installing or running on your system. Use at your own risk.

## Features

- Displays a minimap of your current workspace showing window layout
- Renders as an overlay layer surface (visible over fullscreen windows)
- Click-through design (doesn't intercept mouse events)
- Configurable appearance (colors, borders, gaps, opacity)
- Configurable visibility behavior (always visible or show on events)
- Hot-reloads configuration changes
- Dynamic width based on workspace content

## Screenshots

*Coming soon*

## Installation

### From source

Requires Rust 1.75+ and GTK4 development libraries.

```bash
# Install dependencies (Arch Linux)
sudo pacman -S gtk4 gtk4-layer-shell

# Install dependencies (Fedora)
sudo dnf install gtk4-devel gtk4-layer-shell-devel

# Install dependencies (Ubuntu/Debian)
sudo apt install libgtk-4-dev libgtk4-layer-shell-dev

# Build and install
cargo install --path .

# Build for release
cargo build --release
```

## Usage

Run `nirimap` after starting Niri. For automatic startup, add to your Niri config:

```kdl
spawn-at-startup "nirimap"
```

### Niri Layer Rules

You can add layer rules to customize the minimap's appearance:

```kdl
layer-rule {
    match namespace="nirimap"
    // Add any layer-specific rules here, such as opacity
}
```

## Configuration

Configuration file is located at `~/.config/nirimap/config.toml`. A default configuration is created on first run.

```toml
[display]
height = 100                # Minimap height in pixels (width is dynamic)
max_width_percent = 0.5     # Maximum width as fraction of screen (0.0 - 1.0)
anchor = "top-right"        # Position: top-left, top-center, top-right,
                            #           bottom-left, bottom-center, bottom-right, center
margin_x = 10               # Horizontal margin from edge
margin_y = 10               # Vertical margin from edge

[appearance]
background = "#1e1e2e"    # Background color (hex)
window_color = "#45475a"  # Default window rectangle color
focused_color = "#89b4fa" # Focused window highlight
border_color = "#6c7086"  # Window border color
border_width = 1            # Window border thickness
border_radius = 2           # Corner radius for window rectangles
gap = 2                     # Gap between windows (in minimap pixels)
background_opacity = 0.9    # Background opacity (0.0 = transparent, 1.0 = opaque)

[behavior]
show_on_overview = true     # Keep visible in Niri overview mode (not yet implemented)
always_visible = true       # Always show minimap (false = only on events)
hide_timeout_ms = 2000      # Milliseconds before hiding after an event
```

### Hot Reload

The configuration file is watched for changes. Most settings will apply immediately without restarting:

- Appearance settings (colors, borders, gaps, opacity)
- Behavior settings (visibility, timeout)
- Display settings (height, max width)

**Note**: Changing `anchor` or margins requires restarting nirimap.

### Visibility Behavior

When `always_visible = false`, the minimap will show temporarily when:

- A new window is opened
- Window focus changes (to a different window)
- Workspace is switched
- Window layouts change (resize, move between columns)

The minimap hides automatically after `hide_timeout_ms` milliseconds.

## Known Limitations

### Floating Windows

Floating windows are currently not displayed on the minimap. This is due to a limitation in Niri's IPC API, which doesn't expose viewport scroll position information needed to accurately calculate floating window positions on the minimap.

**Technical Details**: Both tiled and floating windows report viewport-relative coordinates, but the viewport offset cannot be reliably determined from the available IPC data. While we can estimate the viewport offset based on the focused column, this breaks when floating windows have focus or when the viewport scrolls without focus changes (e.g., "center column" operations).

See [Issue #6](https://github.com/alexandergknoll/nirimap/issues/6) for more details and potential future solutions.

## Dependencies

- [niri-ipc](https://crates.io/crates/niri-ipc) - Niri IPC protocol
- [gtk4](https://crates.io/crates/gtk4) - GTK4 bindings
- [gtk4-layer-shell](https://crates.io/crates/gtk4-layer-shell) - Wayland layer shell protocol

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

This is a personal project created as an experiment with AI-assisted development. Issues and pull requests are welcome, but please note the experimental nature of the codebase.
