//! Per-window decorations: text labels and application icons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::cairo::Context;
use gtk4::gdk;
use gtk4::pango;
use gtk4::prelude::*;
use gtk4::IconTheme;

use crate::config::{
    Anchor, Color, Config, FontStyle, FontWeight, IconSize, LabelConfig, LabelContent,
};
use crate::state::Window;

/// Inset from the window edge for icons anchored at a corner/edge.
const ICON_EDGE_INSET: f64 = 2.0;

/// Cap on cached icon lookups. Keys include the client-controlled `app_id`, so
/// a client cycling its app_id could otherwise grow the cache without bound.
/// On overflow the cache is flushed; re-resolving is cheap.
const ICON_CACHE_MAX_ENTRIES: usize = 512;

/// Longest `app_id` worth resolving; real ids are far shorter.
const MAX_APP_ID_LEN: usize = 255;

enum HAlign {
    Left,
    Center,
    Right,
}

enum VAlign {
    Top,
    Center,
    Bottom,
}

fn anchor_h(anchor: Anchor) -> HAlign {
    match anchor {
        Anchor::TopLeft | Anchor::BottomLeft => HAlign::Left,
        Anchor::TopCenter | Anchor::BottomCenter | Anchor::Center => HAlign::Center,
        Anchor::TopRight | Anchor::BottomRight => HAlign::Right,
    }
}

fn anchor_v(anchor: Anchor) -> VAlign {
    match anchor {
        Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => VAlign::Top,
        Anchor::Center => VAlign::Center,
        Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => VAlign::Bottom,
    }
}

/// Draw the configured decorations (icon, then label) for one window rectangle.
#[allow(clippy::too_many_arguments)]
pub fn draw_window_decorations(
    cr: &Context,
    window: &Window,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    config: &Config,
    icon_cache: &mut IconCache,
    widget_scale: i32,
) {
    if config.icons.enabled
        && w >= config.icons.min_window_size
        && h >= config.icons.min_window_size
    {
        draw_window_icon(cr, window, x, y, w, h, config, icon_cache, widget_scale);
    }

    if config.labels.enabled
        && w >= config.labels.min_window_size
        && h >= config.labels.min_window_size
    {
        draw_window_label(cr, window, x, y, w, h, &config.labels);
    }
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/// Label text, falling back to whichever of title/app_id is available.
fn label_text(content: LabelContent, window: &Window) -> Option<String> {
    let title = window.title.as_deref().filter(|s| !s.is_empty());
    let app_id = window.app_id.as_deref().filter(|s| !s.is_empty());

    match content {
        LabelContent::None => None,
        LabelContent::Title => title.or(app_id).map(str::to_string),
        LabelContent::AppId => app_id.or(title).map(str::to_string),
        LabelContent::AppIdTitle => match (app_id, title) {
            (Some(a), Some(t)) => Some(format!("{} — {}", a, t)),
            (Some(a), None) => Some(a.to_string()),
            (None, Some(t)) => Some(t.to_string()),
            (None, None) => None,
        },
    }
}

fn draw_window_label(
    cr: &Context,
    window: &Window,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    labels: &LabelConfig,
) {
    let Some(text) = label_text(labels.content, window) else {
        return;
    };

    let avail_w = w - labels.padding * 2.0;
    if avail_w < 4.0 {
        return;
    }

    let layout = pangocairo::functions::create_layout(cr);
    let mut desc = pango::FontDescription::new();
    desc.set_family(&labels.font_family);
    desc.set_absolute_size(labels.font_size * pango::SCALE as f64);
    desc.set_weight(match labels.font_weight {
        FontWeight::Normal => pango::Weight::Normal,
        FontWeight::Bold => pango::Weight::Bold,
    });
    desc.set_style(match labels.font_style {
        FontStyle::Normal => pango::Style::Normal,
        FontStyle::Italic => pango::Style::Italic,
    });
    layout.set_font_description(Some(&desc));
    layout.set_text(&text);
    layout.set_width((avail_w * pango::SCALE as f64) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_alignment(match anchor_h(labels.position) {
        HAlign::Left => pango::Alignment::Left,
        HAlign::Center => pango::Alignment::Center,
        HAlign::Right => pango::Alignment::Right,
    });

    let (_, text_h) = layout.pixel_size();
    let text_h = text_h as f64;
    let ty = match anchor_v(labels.position) {
        VAlign::Top => y + labels.padding,
        VAlign::Center => y + (h - text_h) / 2.0,
        VAlign::Bottom => y + h - text_h - labels.padding,
    };
    let tx = x + labels.padding;

    let hex = if window.is_focused {
        &labels.focused_color
    } else {
        &labels.color
    };
    let color = Color::from_hex(hex).unwrap_or(Color {
        r: 0.8,
        g: 0.84,
        b: 0.96,
        a: 1.0,
    });

    cr.save().ok();
    cr.rectangle(x, y, w, h);
    cr.clip();

    if labels.shadow {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
        cr.move_to(tx + 1.0, ty + 1.0);
        pangocairo::functions::show_layout(cr, &layout);
    }

    cr.set_source_rgba(color.r, color.g, color.b, color.a);
    cr.move_to(tx, ty);
    pangocairo::functions::show_layout(cr, &layout);

    cr.restore().ok();
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// Resolve the icon size in pixels for a window rectangle. Quantized to even
/// integers so cache keys stay stable as the minimap rescales.
fn resolve_icon_size(size: IconSize, w: f64, h: f64) -> f64 {
    let max_fit = w.min(h);
    let ideal = match size {
        IconSize::Auto => (max_fit * 0.6).clamp(8.0, 64.0),
        IconSize::Pixels(px) => px,
    };
    ((ideal.min(max_fit) / 2.0).round() * 2.0).max(2.0)
}

#[allow(clippy::too_many_arguments)]
fn draw_window_icon(
    cr: &Context,
    window: &Window,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    config: &Config,
    icon_cache: &mut IconCache,
    widget_scale: i32,
) {
    let Some(app_id) = window.app_id.as_deref().filter(|s| !s.is_empty()) else {
        return;
    };

    let icons = &config.icons;
    let size = resolve_icon_size(icons.size, w, h);
    if size < 4.0 {
        return;
    }

    let ix = match anchor_h(icons.position) {
        HAlign::Left => x + ICON_EDGE_INSET,
        HAlign::Center => x + (w - size) / 2.0,
        HAlign::Right => x + w - size - ICON_EDGE_INSET,
    };
    let iy = match anchor_v(icons.position) {
        VAlign::Top => y + ICON_EDGE_INSET,
        VAlign::Center => y + (h - size) / 2.0,
        VAlign::Bottom => y + h - size - ICON_EDGE_INSET,
    };

    let opacity = icons.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }

    cr.save().ok();
    cr.rectangle(x, y, w, h);
    cr.clip();

    match icon_cache.lookup(
        app_id,
        size as i32,
        widget_scale.max(1),
        icons.theme_override.as_deref(),
    ) {
        Some(paintable) => draw_paintable(cr, &paintable, ix, iy, size, opacity),
        None => draw_letter_fallback(cr, app_id, ix, iy, size, opacity),
    }

    cr.restore().ok();
}

fn draw_paintable(
    cr: &Context,
    paintable: &gdk::Paintable,
    x: f64,
    y: f64,
    size: f64,
    opacity: f64,
) {
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(&snapshot, size, size);
    let Some(node) = snapshot.to_node() else {
        return;
    };

    cr.save().ok();
    cr.translate(x, y);
    if opacity < 1.0 {
        cr.push_group();
        node.draw(cr);
        if cr.pop_group_to_source().is_ok() {
            cr.paint_with_alpha(opacity).ok();
        }
    } else {
        node.draw(cr);
    }
    cr.restore().ok();
}

/// Last-resort icon: the first character of the app_id in a circle.
fn draw_letter_fallback(cr: &Context, app_id: &str, x: f64, y: f64, size: f64, opacity: f64) {
    let Some(letter) = app_id
        .rsplit('.')
        .next()
        .and_then(|s| s.chars().next())
        .map(|c| c.to_uppercase().to_string())
    else {
        return;
    };

    let cx = x + size / 2.0;
    let cy = y + size / 2.0;

    cr.set_source_rgba(0.45, 0.47, 0.55, 0.9 * opacity);
    cr.new_path();
    cr.arc(cx, cy, size / 2.0, 0.0, std::f64::consts::TAU);
    cr.fill().ok();

    let layout = pangocairo::functions::create_layout(cr);
    let mut desc = pango::FontDescription::new();
    desc.set_family("Sans");
    desc.set_weight(pango::Weight::Bold);
    desc.set_absolute_size(size * 0.55 * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(&letter);

    let (tw, th) = layout.pixel_size();
    cr.set_source_rgba(1.0, 1.0, 1.0, opacity);
    cr.move_to(cx - tw as f64 / 2.0, cy - th as f64 / 2.0);
    pangocairo::functions::show_layout(cr, &layout);
}

/// Cache key: app_id + requested pixel size + display scale.
type IconKey = (String, i32, i32);

/// Caches resolved icons and the desktop-file index so lookups don't run
/// every frame.
#[derive(Default)]
pub struct IconCache {
    /// `None` = resolution failed; the letter fallback is drawn instead.
    icons: HashMap<IconKey, Option<gdk::Paintable>>,
    /// Lazily built: lowercased StartupWMClass / desktop-file stem -> Icon.
    desktop_icon_map: Option<HashMap<String, String>>,
    /// Icon theme and the override name it was built for.
    theme: Option<(Option<String>, IconTheme)>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called on config reload so `theme_override` changes take effect.
    pub fn clear(&mut self) {
        self.icons.clear();
        self.desktop_icon_map = None;
        self.theme = None;
    }

    /// Icon for `app_id`, or `None` (the caller draws a letter fallback). Tries
    /// the app_id as a themed icon name, then the desktop-file index.
    pub fn lookup(
        &mut self,
        app_id: &str,
        size: i32,
        scale: i32,
        theme_override: Option<&str>,
    ) -> Option<gdk::Paintable> {
        if app_id.len() > MAX_APP_ID_LEN {
            return None;
        }

        let key = (app_id.to_string(), size, scale);
        if let Some(cached) = self.icons.get(&key) {
            return cached.clone();
        }

        let resolved = self.resolve(app_id, size, scale, theme_override);

        if self.icons.len() >= ICON_CACHE_MAX_ENTRIES {
            tracing::debug!("Icon cache reached {} entries, flushing", self.icons.len());
            self.icons.clear();
        }
        self.icons.insert(key, resolved.clone());
        resolved
    }

    fn resolve(
        &mut self,
        app_id: &str,
        size: i32,
        scale: i32,
        theme_override: Option<&str>,
    ) -> Option<gdk::Paintable> {
        let theme = self.theme(theme_override)?.clone();

        let lowercase = app_id.to_lowercase();

        // Only forward icon-name-shaped app_ids to the theme; the desktop-file
        // map below still handles the rest (e.g. app_ids with spaces).
        if is_plain_icon_name(app_id) {
            for name in [app_id, lowercase.as_str()] {
                if theme.has_icon(name) {
                    return Some(self.themed_icon(&theme, name, size, scale));
                }
            }
        }

        // Desktop-file fallback (how launchers resolve e.g. Electron apps).
        let icon_name = self
            .desktop_icon_map
            .get_or_insert_with(build_desktop_icon_map)
            .get(&lowercase)?
            .clone();

        // Desktop entries may give an absolute path instead of a themed name.
        if icon_name.starts_with('/') {
            let file = gtk4::gio::File::for_path(&icon_name);
            return gdk::Texture::from_file(&file).ok().map(|t| t.upcast());
        }

        if theme.has_icon(&icon_name) {
            return Some(self.themed_icon(&theme, &icon_name, size, scale));
        }

        None
    }

    fn themed_icon(&self, theme: &IconTheme, name: &str, size: i32, scale: i32) -> gdk::Paintable {
        theme
            .lookup_icon(
                name,
                &[],
                size,
                scale,
                gtk4::TextDirection::None,
                gtk4::IconLookupFlags::empty(),
            )
            .upcast()
    }

    /// Icon theme for `theme_override`, rebuilt when the override changes.
    fn theme(&mut self, theme_override: Option<&str>) -> Option<&IconTheme> {
        let wanted = theme_override.map(str::to_string);
        let stale = match &self.theme {
            Some((cached_override, _)) => *cached_override != wanted,
            None => true,
        };

        if stale {
            let theme = match theme_override {
                Some(name) => {
                    let theme = IconTheme::new();
                    theme.set_theme_name(Some(name));
                    theme
                }
                None => IconTheme::for_display(&gdk::Display::default()?),
            };
            self.theme = Some((wanted, theme));
        }

        self.theme.as_ref().map(|(_, t)| t)
    }
}

/// Whether `name` is safe to hand to the icon theme: `[A-Za-z0-9._-]` only.
/// app_id is client-controlled, so paths, whitespace, and control characters
/// are rejected.
fn is_plain_icon_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_APP_ID_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

// ---------------------------------------------------------------------------
// Desktop-file fallback index
// ---------------------------------------------------------------------------

/// XDG data directories that may contain `applications/*.desktop`.
fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(dirs::data_dir)
    {
        dirs.push(data_home);
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }

    dirs
}

/// Index desktop entries by lowercased StartupWMClass and file stem -> Icon.
/// Earlier (user) directories win over later (system) ones.
fn build_desktop_icon_map() -> HashMap<String, String> {
    let mut map = HashMap::new();

    for dir in xdg_data_dirs() {
        let apps_dir = dir.join("applications");
        let Ok(entries) = std::fs::read_dir(&apps_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            index_desktop_entry(&mut map, &path, &contents);
        }
    }

    map
}

/// Add one desktop entry's icon to the index (first writer wins).
fn index_desktop_entry(map: &mut HashMap<String, String>, path: &Path, contents: &str) {
    let (wm_class, icon) = parse_desktop_entry(contents);
    let Some(icon) = icon else {
        return;
    };

    if let Some(wm_class) = wm_class {
        map.entry(wm_class.to_lowercase())
            .or_insert_with(|| icon.clone());
    }
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        map.entry(stem.to_lowercase()).or_insert(icon);
    }
}

/// (StartupWMClass, Icon) from the `[Desktop Entry]` section.
fn parse_desktop_entry(contents: &str) -> (Option<String>, Option<String>) {
    let mut in_desktop_entry = false;
    let mut wm_class = None;
    let mut icon = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(value) = line.strip_prefix("StartupWMClass=") {
            wm_class.get_or_insert_with(|| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("Icon=") {
            icon.get_or_insert_with(|| value.trim().to_string());
        }
    }

    (wm_class, icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window_with(title: Option<&str>, app_id: Option<&str>) -> Window {
        Window {
            id: 1,
            pos: None,
            size: (100.0, 100.0),
            column_index: 0,
            window_index: 0,
            is_focused: false,
            is_floating: false,
            title: title.map(str::to_string),
            app_id: app_id.map(str::to_string),
        }
    }

    #[test]
    fn test_label_text_title() {
        let w = window_with(Some("GitHub"), Some("firefox"));
        assert_eq!(
            label_text(LabelContent::Title, &w),
            Some("GitHub".to_string())
        );
    }

    #[test]
    fn test_label_text_title_falls_back_to_app_id() {
        let w = window_with(None, Some("firefox"));
        assert_eq!(
            label_text(LabelContent::Title, &w),
            Some("firefox".to_string())
        );
    }

    #[test]
    fn test_label_text_app_id_title() {
        let w = window_with(Some("GitHub"), Some("firefox"));
        assert_eq!(
            label_text(LabelContent::AppIdTitle, &w),
            Some("firefox — GitHub".to_string())
        );
    }

    #[test]
    fn test_label_text_none() {
        let w = window_with(Some("GitHub"), Some("firefox"));
        assert_eq!(label_text(LabelContent::None, &w), None);
    }

    #[test]
    fn test_label_text_empty_strings_treated_as_missing() {
        let w = window_with(Some(""), Some(""));
        assert_eq!(label_text(LabelContent::Title, &w), None);
        assert_eq!(label_text(LabelContent::AppIdTitle, &w), None);
    }

    #[test]
    fn test_resolve_icon_size_auto_scales_with_rect() {
        // 60% of the smaller dimension, quantized to even integers
        assert_eq!(resolve_icon_size(IconSize::Auto, 40.0, 60.0), 24.0);
        // min 8
        assert_eq!(resolve_icon_size(IconSize::Auto, 10.0, 10.0), 8.0);
        // max 64
        assert_eq!(resolve_icon_size(IconSize::Auto, 500.0, 500.0), 64.0);
    }

    #[test]
    fn test_resolve_icon_size_pixels_capped_by_rect() {
        assert_eq!(
            resolve_icon_size(IconSize::Pixels(24.0), 100.0, 100.0),
            24.0
        );
        assert_eq!(resolve_icon_size(IconSize::Pixels(24.0), 12.0, 100.0), 12.0);
    }

    #[test]
    fn test_is_plain_icon_name_accepts_typical_app_ids() {
        for name in [
            "firefox",
            "org.gnome.Nautilus",
            "com.mitchellh.ghostty",
            "code-url-handler",
            "Alacritty",
            "steam_app_1234",
        ] {
            assert!(is_plain_icon_name(name), "{name:?} should be accepted");
        }
    }

    #[test]
    fn test_is_plain_icon_name_rejects_paths_whitespace_and_control() {
        for name in [
            "",
            "../../etc/passwd",
            "/usr/share/icons/x.png",
            "Microsoft Teams",
            "evil\ntitle",
            "app\0id",
            "tab\tapp",
            "ünïcödé",
        ] {
            assert!(!is_plain_icon_name(name), "{name:?} should be rejected");
        }
    }

    #[test]
    fn test_is_plain_icon_name_rejects_overlong() {
        let ok = "a".repeat(MAX_APP_ID_LEN);
        let too_long = "a".repeat(MAX_APP_ID_LEN + 1);
        assert!(is_plain_icon_name(&ok));
        assert!(!is_plain_icon_name(&too_long));
    }

    #[test]
    fn test_parse_desktop_entry() {
        let contents = "\
[Desktop Entry]
Name=Firefox
Icon=firefox
StartupWMClass=firefox

[Desktop Action new-window]
Icon=other-icon
";
        let (wm_class, icon) = parse_desktop_entry(contents);
        assert_eq!(wm_class.as_deref(), Some("firefox"));
        // The [Desktop Action] section's Icon must not override the main one
        assert_eq!(icon.as_deref(), Some("firefox"));
    }

    #[test]
    fn test_parse_desktop_entry_missing_keys() {
        let contents = "[Desktop Entry]\nName=Thing\n";
        let (wm_class, icon) = parse_desktop_entry(contents);
        assert_eq!(wm_class, None);
        assert_eq!(icon, None);
    }

    #[test]
    fn test_index_desktop_entry_maps_wm_class_and_stem() {
        let mut map = HashMap::new();
        let contents = "[Desktop Entry]\nIcon=code\nStartupWMClass=Code\n";
        index_desktop_entry(
            &mut map,
            Path::new("/usr/share/applications/visual-studio-code.desktop"),
            contents,
        );
        assert_eq!(map.get("code").map(String::as_str), Some("code"));
        assert_eq!(
            map.get("visual-studio-code").map(String::as_str),
            Some("code")
        );
    }
}
