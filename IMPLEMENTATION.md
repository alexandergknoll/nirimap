# Implementing a Minimap for Niri: Technical Implementation Guide

A scrolling minimap feature for Niri is **fully achievable as a standalone Wayland application**, leveraging Niri's robust IPC system and layer-shell protocol support. Niri does not have a plugin system, but its external tool integration is comprehensive enough that a minimap can access all necessary window layout data and render as a persistent overlay. The primary implementation path combines **niri-ipc for state queries** with **wlr-layer-shell for rendering** and either **wlr-screencopy or PipeWire for thumbnails**.

## Niri exposes everything needed via IPC

Niri provides a **JSON-based IPC system** through a Unix domain socket at `$NIRI_SOCKET`. This is the same mechanism Waybar uses for its native `niri/workspaces` and `niri/window` modules. The protocol supports both one-shot queries and continuous event streaming—critical for real-time minimap updates.

**Window layout data** is available in remarkable detail. Each window includes `pos_in_scrolling_layout: (column_index, window_index_in_column)`, tile position in workspace view coordinates, and both tile and window sizes in logical pixels. This data maps directly to minimap visualization needs:

```json
{
  "id": 12,
  "title": "Firefox",
  "app_id": "firefox",
  "workspace_id": 6,
  "layout": {
    "pos_in_scrolling_layout": [2, 0],
    "tile_size": [960.0, 1040.0],
    "tile_pos_in_workspace_view": [480.0, 0.0],
    "window_offset_in_tile": [4.0, 4.0]
  }
}
```

The **event stream** (`niri msg event-stream`) eliminates polling by pushing state changes as they occur. Key events include `WindowOpenedOrChanged`, `WindowClosed`, `WindowFocusChanged`, and `WorkspaceActivated`. The stream provides complete initial state on connection, then incremental updates—designed specifically for tools like status bars and, by extension, a minimap.

## Layer-shell protocol enables overlay rendering

Niri fully supports **zwlr_layer_shell_v1**, the same protocol Waybar uses to render as a panel. For a minimap, the **overlay layer** is essential—it remains visible over fullscreen windows and Niri's built-in Overview mode. The implementation should use `gtk-layer-shell` (GTK3/4 wrapper) or `layer-shell-qt` (Qt wrapper) rather than implementing the protocol directly:

| Layer | Behavior | Minimap Suitability |
|-------|----------|---------------------|
| background | Below everything, zooms with overview | ❌ |
| bottom | Below windows | ❌ |
| top | Above windows, hidden by fullscreen | ⚠️ Partial |
| **overlay** | Always visible, above fullscreen | ✅ Recommended |

Configure exclusive zone to **0** so the minimap doesn't reserve screen space. Anchor to a screen edge (top-right is typical for minimaps) and set keyboard interactivity to "none" or "on-demand" based on whether the minimap should be interactive.

## Window thumbnails require strategic protocol choices

Capturing actual window content for thumbnails is the most complex aspect. Niri's current protocol support offers two viable paths:

**Option 1: wlr-screencopy (output-level capture)**
Niri implements wlr-screencopy v3 with damage tracking and DMA-BUF support. This captures entire outputs or rectangular regions but **cannot capture individual windows directly**. For a minimap showing workspace overview rather than per-window thumbnails, this works well:

```rust
// Simplified flow using wayland-protocols-wlr crate
let frame = screencopy_manager.capture_output(overlay_cursor, output);
// Create DMA-BUF buffer from format events
frame.copy_with_damage(buffer); // Wait for changes before copying
// Ready event provides captured frame
```

**Option 2: PipeWire portal (individual window capture)**
Niri supports PipeWire-based screencasting for individual windows through xdg-desktop-portal. This is how OBS and screen sharing in browsers capture specific windows. Use `niri msg action set-dynamic-cast-window` to target specific windows programmatically.

**Missing protocol: ext-image-capture-source-v1**
The newer `ext-foreign-toplevel-image-capture-source-v1` protocol would enable direct per-window capture without portal overhead, but **Niri does not yet implement this**. Filing a feature request could be worthwhile for the project's long-term benefit.

| Method | Individual Windows | Performance | Niri Support |
|--------|-------------------|-------------|--------------|
| wlr-screencopy | ❌ (regions only) | Excellent w/ DMA-BUF | ✅ v3 |
| PipeWire portal | ✅ | Good | ✅ |
| ext-image-capture-source | ✅ | Excellent | ❌ Not yet |

## No plugin system exists, but IPC is sufficient

Niri explicitly **does not have a plugin system** and there are no public plans to add one. This contrasts with Hyprland, which offers a full C++ plugin API with runtime-loadable shared libraries. Niri's design philosophy prioritizes a clean, maintainable Rust codebase over internal extensibility.

However, the IPC system is intentionally robust enough to replace most plugin use cases. The **134 available actions** cover focus management, window manipulation, workspace control, and more—all invocable from external applications:

```bash
# Focus a specific window by ID
niri msg action focus-window --id=12

# Move focus to adjacent columns (minimap navigation)
niri msg action focus-column-left
niri msg action focus-column-right

# Switch workspaces
niri msg action focus-workspace 2
```

Community tools demonstrate this approach's viability: `niri-switch` implements an alt-tab task switcher, `niriswitcher` provides application switching with workspace support, and various eww/Waybar modules render workspace indicators—all using IPC without compositor modification.

## Understanding Niri's scrolling workspace model

Niri's layout paradigm differs fundamentally from traditional tiling window managers and directly influences minimap design. Windows exist on an **infinite horizontal strip** that scrolls left/right, with the visible screen acting as a viewport into this strip. Windows are organized into **columns**, with each column containing one or more vertically-stacked windows:

```
┌─────────────────────── Infinite Strip ──────────────────────┐
│ [Col 1] [Col 2]  [Col 3]  [Col 4]  [Col 5] [Col 6] ...     │
│ [Win A] [Win B]  [Win D]  [Win F]  [Win G] [Win H]         │
│         [Win C]  [Win E]                                    │
│      ◄──── Visible Viewport ────►                          │
└─────────────────────────────────────────────────────────────┘
```

**Workspaces are vertical**, arranged per-monitor with dynamic creation/destruction. The minimap should visualize both the horizontal column layout within the current workspace and potentially the vertical workspace stack. The `tile_pos_in_workspace_view` coordinates from IPC use the same coordinate space as Niri's internal rendering, enabling accurate minimap positioning.

One gap in current IPC: the **exact viewport scroll offset** isn't directly exposed. The minimap can infer viewport position from focused column data and calculated column widths, or this could be requested as a future IPC enhancement.

## Recommended implementation architecture

**Language choice: Rust** is strongly recommended. The `niri-ipc` crate provides type-safe request/response handling with `Socket` helpers, eliminating JSON parsing boilerplate. GTK4 with `gtk4-layer-shell` offers mature layer-shell integration, though a pure Wayland client using `smithay-client-toolkit` would be lighter weight.

**Core components:**

1. **IPC Client Module**
   - Connect to `$NIRI_SOCKET`
   - Subscribe to event stream for real-time updates
   - Query full state on connection, maintain local model

2. **State Model**
   - Track columns, windows-per-column, sizes, and positions
   - Track focused window/column/workspace
   - Infer viewport position from focus and layout data

3. **Layer-Shell Surface**
   - Render on overlay layer for fullscreen visibility
   - Configurable anchor position and size
   - Optional click-to-focus interaction

4. **Thumbnail Engine (optional)**
   - PipeWire capture for individual window previews
   - wlr-screencopy for workspace-level overview
   - Consider falling back to app icons if capture unavailable

**Configuration integration:**
```kdl
// User's ~/.config/niri/config.kdl
spawn-at-startup "niri-minimap"

layer-rule {
    match namespace="niri-minimap"
    opacity 0.9
    shadow { on; softness 10; }
}
```

## Key reference implementations to study

Several existing projects demonstrate relevant patterns:

- **wl-screenrec** (Rust): High-performance DMA-BUF screencopy with both wlr-screencopy and ext-image-copy-capture backends
- **grim** (C): Reference wlr-screencopy implementation
- **Waybar niri modules**: IPC event stream consumption patterns
- **niriswitcher**: Application switcher using niri IPC
- **eww-niri-workspaces**: Eww widget consuming niri workspace state
- **niri-ipc crate**: Official Rust bindings with examples

The `niri-ipc` crate documentation at `https://docs.rs/niri-ipc/` provides complete type definitions for all requests, responses, and events—essential reference for implementation.

## Conclusion

Building a minimap for Niri is architecturally straightforward despite the absence of a plugin system. The combination of **comprehensive IPC with event streaming**, **full layer-shell support**, and **wlr-screencopy/PipeWire for thumbnails** provides all necessary primitives. The main implementation challenges are inferring viewport scroll position (work around with column position data) and per-window thumbnails (use PipeWire until ext-image-capture-source is implemented). A Rust implementation using `niri-ipc` and `gtk4-layer-shell` would integrate seamlessly with Niri's ecosystem and could serve as a valuable community contribution to the growing `awesome-niri` collection.