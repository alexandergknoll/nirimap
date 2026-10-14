# Claude Code Context

This file provides context for Claude when working on this project.

## Project Overview

nirimap is a workspace minimap overlay for the Niri Wayland compositor. It displays a visual representation of the current workspace's window layout as colored rectangles.

## Architecture

```
src/
├── main.rs           # Entry point, GTK app setup, event loop integration
├── config.rs         # Configuration loading/parsing from TOML
├── config_watcher.rs # Config file watching with trailing-edge debounce
├── ipc/
│   ├── mod.rs        # Module exports
│   ├── client.rs     # Niri socket connection for queries
│   └── events.rs     # Event stream handling and state updates
├── state/
│   ├── mod.rs        # Module exports
│   └── model.rs      # Window/workspace state data structures
└── ui/
    ├── mod.rs        # Module exports
    ├── layer.rs      # GTK4 layer-shell window setup
    ├── minimap.rs    # Cairo drawing and widget logic
    ├── draw.rs       # Per-frame DrawCtx and the Palette of parsed colours
    └── decorations.rs # Window labels (PangoCairo) and app icons (IconTheme + cache)
```

## Key Technical Details

### IPC Communication
- Uses `niri-ipc` crate to communicate with Niri via Unix socket (`$NIRI_SOCKET`)
- Subscribes to event stream for real-time updates
- The IPC thread sends `StateUpdate`s over an `async-channel`; the GTK main loop awaits them, then drains and coalesces any backlog in batches at most every 50 ms
- Key events: `WindowOpenedOrChanged`, `WindowClosed`, `WindowFocusChanged`, `WorkspaceActivated`, `WindowLayoutsChanged`

### Rendering
- GTK4 `DrawingArea` with Cairo for custom drawing
- `gtk4-layer-shell` for Wayland layer surface (overlay layer)
- Click-through via empty Wayland input region
- Config colours are parsed once per load into a `Palette` (invalid values log a warning and use fallbacks)
- Dynamic width sizing based on workspace content

### Configuration
- TOML config at `~/.config/nirimap/config.toml`
- Hot reload via `notify` crate file watching, applied once the file has been unchanged for 500 ms
- Config stored in `Rc<RefCell<Config>>` for runtime updates

### Visibility Control
- Tracks last focused window to prevent duplicate show events
- Distinguishes between new windows and property updates
- Auto-hide with configurable timeout when `always_visible = false`

## Common Tasks

### Adding a new config option
1. Add field to appropriate struct in `src/config.rs` (DisplayConfig, AppearanceConfig, or BehaviorConfig)
2. Add default value in the `Default` impl
3. Update `DEFAULT_CONFIG_TOML`
4. Use the new field where needed (access via `config.borrow()`); colours go through `Palette` in `src/ui/draw.rs`

### Adding a new event handler
1. Add variant to `StateUpdate` enum in `src/ipc/events.rs`
2. Handle the niri event in `event_to_update()`
3. Process the update in `apply_state_update()` in `src/main.rs`, putting state mutation in a `MinimapState` method so it can be unit-tested

### Modifying the minimap rendering
- Drawing logic is in `draw_minimap()` in `src/ui/minimap.rs`, which dispatches to `draw_current_workspace()` or `draw_all_workspaces()`; both draw windows via `draw_tiled_windows()`
- Window positioning calculated from `column_index` and `window_index`
- Scale factor based on fitting workspace height into minimap height

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build
cargo install --path .   # Install to ~/.cargo/bin
```

## Testing

`cargo test` covers config parsing, state updates, update coalescing, the reload debounce and decoration helpers. Rendering, layer-shell and visibility behaviour still need manual testing with Niri.

To test manually:
1. Run `nirimap` in a terminal to see logs
2. Test window operations (open, close, focus, resize, move)
3. Test workspace switching
4. Test config changes (should hot reload)
