//! Playback overlay state, rendering, layout, and pointer interaction.

mod acrylic;
mod controls;
mod help;
mod interaction;
mod layout;
mod panels;
mod playback_overlay;
mod playlist;
mod raster;
mod rendering;
mod state;
mod style;
mod text;
mod timeline;

pub(crate) use playback_overlay::PlaybackOverlay;
pub(crate) use state::{
    AudioPickerAction, HitboxRect, MediaInfo, MediaInfoState, OverlayHitContext, OverlayHitPoint,
    OverlayRenderContext, OverlayState, PlaylistMenuAction, PlaylistMenuState,
    SubtitlePickerAction, TransportControlAction,
};

#[cfg(test)]
use playback_overlay::render_overlay_rgb;
#[cfg(test)]
use rendering::render_overlay_rgb as render_overlay_rgb_with_palette;

#[cfg(test)]
#[path = "tests/playback_overlay.rs"]
mod tests;
