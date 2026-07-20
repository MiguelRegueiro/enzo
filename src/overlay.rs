//! Playback overlay state, rendering, layout, and pointer interaction.
//!
//! This facade keeps the rest of the application independent from the
//! overlay's drawing implementation. Rendering and interaction share the same
//! layout model so visible controls and their pointer targets stay aligned.

mod acrylic;
mod controls;
mod interaction;
mod layout;
mod panels;
mod raster;
mod rendering;
mod state;
mod style;
mod text;
mod timeline;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use acrylic::AcrylicScratch;
use interaction::{
    audio_picker_action, playback_button_hit, progress_hit_ratio, progress_ratio_for_x,
    subtitle_picker_action,
};
use layout::{
    OverlayMetrics, overlay_metrics, track_picker_layout, track_picker_visible_row_count,
};
use rendering::render_overlay_rgb;

use crate::{
    font::FontRenderer,
    font_system::{FontRole, FontSystem},
};

pub(crate) use state::{
    AudioPickerAction, HitboxRect, MediaInfo, MediaInfoState, OverlayHitContext, OverlayHitPoint,
    OverlayState, SubtitlePickerAction,
};

pub(crate) struct PlaybackOverlay {
    scratch: String,
    acrylic: AcrylicScratch,
    font: Option<FontRenderer>,
}

impl PlaybackOverlay {
    pub(crate) fn new(fonts: &FontSystem) -> Self {
        let mut font = fonts
            .resolve_all(FontRole::Ui)
            .find_map(|path| FontRenderer::open_path(path, 18));
        if let Some(font) = font.as_mut() {
            for language in ["zh", "ja"] {
                for path in fonts.resolve_all_for_language(FontRole::Subtitle, Some(language)) {
                    if font.add_fallback_path(&path) {
                        break;
                    }
                }
            }
        }
        Self {
            scratch: String::new(),
            acrylic: AcrylicScratch::default(),
            font,
        }
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        terminal_rows: u16,
        scale_percent: u32,
        state: OverlayState,
    ) {
        render_overlay_rgb(
            frame,
            width,
            height,
            terminal_rows,
            scale_percent,
            state,
            &mut self.scratch,
            &mut self.acrylic,
            self.font.as_mut(),
        );
    }

    pub(crate) fn progress_hit_test(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
    ) -> Option<f64> {
        let metrics = self.metrics(context);
        progress_hit_ratio(metrics, point, context.position, context.duration)
    }

    pub(crate) fn progress_ratio_from_x(&mut self, context: OverlayHitContext, x: u32) -> f64 {
        let metrics = self.metrics(context);
        progress_ratio_for_x(metrics, x)
    }

    pub(crate) fn playback_button_hit_test(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
    ) -> bool {
        let metrics = self.metrics(context);
        playback_button_hit(metrics, point)
    }

    pub(crate) fn audio_picker_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<AudioPickerAction> {
        let metrics = self.metrics(context);
        let row_count = labels.len();
        let visible_count = track_picker_visible_row_count(metrics, row_count);
        let scroll_offset = scroll_offset.min(row_count.saturating_sub(visible_count));
        let picker = picker_open.then(|| {
            track_picker_layout(metrics, labels, false, scroll_offset, self.font.as_mut())
        });
        audio_picker_action(
            metrics,
            point,
            picker,
            row_count,
            scroll_offset,
            visible_count,
        )
    }

    pub(crate) fn subtitle_picker_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<SubtitlePickerAction> {
        let metrics = self.metrics(context);
        let row_count = labels.len().saturating_add(1);
        let visible_count = track_picker_visible_row_count(metrics, row_count);
        let scroll_offset = scroll_offset.min(row_count.saturating_sub(visible_count));
        let picker = picker_open
            .then(|| track_picker_layout(metrics, labels, true, scroll_offset, self.font.as_mut()));
        subtitle_picker_action(
            metrics,
            point,
            picker,
            labels.len(),
            scroll_offset,
            visible_count,
        )
    }

    pub(crate) fn track_picker_visible_row_count(
        &mut self,
        context: OverlayHitContext,
        row_count: usize,
    ) -> usize {
        let metrics = self.metrics(context);
        track_picker_visible_row_count(metrics, row_count)
    }

    fn metrics(&mut self, context: OverlayHitContext) -> OverlayMetrics {
        overlay_metrics(
            context.width,
            context.height,
            context.terminal_rows,
            context.scale_percent,
            context.duration,
            context.audio_available,
            context.subtitles_available,
            self.font.as_mut(),
        )
    }
}
