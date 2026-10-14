//! Resources shared by everything drawn in one frame.

use gtk4::cairo::Context;

use super::decorations::IconCache;
use crate::config::Config;

pub(super) struct DrawCtx<'a> {
    pub cr: &'a Context,
    pub config: &'a Config,
    pub icon_cache: &'a mut IconCache,
    /// Display scale factor, so icons are rasterized at device resolution.
    pub widget_scale: i32,
}
