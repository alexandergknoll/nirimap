# nirimap

[![CI](https://github.com/alexandergknoll/nirimap/actions/workflows/ci.yml/badge.svg)](https://github.com/alexandergknoll/nirimap/actions/workflows/ci.yml)

A minimal workspace minimap overlay for the [Niri](https://github.com/YaLTeR/niri) Wayland compositor.

## Features

- Displays a minimap of your workspaces showing window layout
- Two display modes: show every workspace stacked vertically (Overview-style) or only the active one
- Renders as an overlay layer surface (visible over fullscreen windows)
- Click-through design (doesn't intercept mouse events)
- Configurable appearance (colors, borders, gaps, opacity)
- Configurable visibility behavior (always visible or show on events)
- Hot-reloads configuration changes
- Dynamic sizing based on workspace content

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
height = 100                # Per-workspace row height in pixels
                            # "current" mode: whole widget height
                            # "all" mode: height of a single workspace row
max_width_percent = 0.5     # Maximum width as fraction of screen (0.0 - 1.0)
max_height_percent = 0.8    # Maximum height as fraction of screen ("all" mode)
anchor = "top-right"        # Position: top-left, top-center, top-right,
                            #           bottom-left, bottom-center, bottom-right, center
margin_x = 10               # Horizontal margin from edge
margin_y = 10               # Vertical margin from edge
workspace_mode = "all"      # "all"     - stack every workspace vertically (default)
                            # "current" - show only the active workspace

[appearance]
background = "#1e1e2e"    # Background color (hex)
window_color = "#45475a"  # Default window rectangle color
focused_color = "#89b4fa" # Focused window highlight
border_color = "#6c7086"  # Window border color
border_width = 1            # Window border thickness
border_radius = 2           # Corner radius for window rectangles
gap = 2                     # Gap between windows (in minimap pixels)
background_opacity = 0.0    # Background opacity (0.0 = transparent, 1.0 = opaque)
                            # Applies in both "current" and "all" modes
window_opacity = 0.7        # Fill opacity for unfocused windows (0 = outlines only)
focused_opacity = 1.0       # Fill opacity for the focused window
workspace_gap = 4                           # Vertical gap between stacked workspaces ("all" mode)
active_workspace_border_color = "#89b4fa"   # Highlight border for the active workspace ("all" mode)
active_workspace_border_width = 2           # Highlight border thickness ("all" mode)

[behavior]
show_on_overview = true        # Keep visible in Niri overview mode (not yet implemented)
always_visible = true          # Always show minimap (false = only on events)
hide_timeout_ms = 2000         # Milliseconds before hiding after an event
show_for_floating_windows = false # Surface the minimap for floating-window events
                                  # (focus to/from a floating window, floating window
                                  # spawn). Off by default — floating windows aren't
                                  # drawn on the minimap, so popup activity would
                                  # otherwise flash it on/off.
```

### Workspace Display Modes

Two display modes control what the minimap shows:

- **`all`** (default) — every workspace is rendered as a row, stacked vertically in Niri's workspace order (like Niri's Overview feature). The active workspace is highlighted with a border so you can see where focus is at a glance.
- **`current`** — only the active workspace is rendered. The widget height equals `display.height` and the minimap content changes as you switch workspaces. This is the classic nirimap behavior.

In `all` mode the total widget height grows with the number of workspaces, capped at `max_height_percent` of the monitor's height. When the cap is hit, per-workspace rows shrink proportionally to fit.

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

By default, the minimap stays hidden for floating-window activity:

- Focus moving **to** a floating window (popup, dialog, file picker)
- Focus returning **from** a floating window to the previously-focused tile
- A new floating window being spawned

Floating windows aren't drawn on the minimap, so this activity would
otherwise cause a distracting on/off flash. Set
`show_for_floating_windows = true` to restore the prior behavior.

## Known Limitations

### Multi-Monitor Support

nirimap currently only tracks and displays windows on a single monitor. Multi-monitor setups may result in windows not appearing on the minimap or incorrect workspace tracking. Full multi-monitor support is planned.

See [Issue #21](https://github.com/alexandergknoll/nirimap/issues/21) for more details.

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

Issues and pull requests are welcome! This project was developed with the help of AI-assisted tooling (Claude Code) — please review changes carefully and feel free to flag anything that looks off.
