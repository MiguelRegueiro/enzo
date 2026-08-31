use std::sync::Arc;

use crate::font::{FontRenderer, FontRole, FontSystem};

use super::{
    acrylic::AcrylicScratch,
    help::help_scroll_limit,
    interaction::{
        audio_picker_action, progress_hit_ratio, progress_ratio_for_x, subtitle_picker_action,
        track_picker_hover_index, transport_control_action,
    },
    layout::{
        OverlayMetrics, overlay_metrics, track_picker_layout, track_picker_visible_row_count,
    },
    playlist::{playlist_menu_action, playlist_menu_hover_index, playlist_menu_visible_row_count},
    rendering::render_overlay_rgb as render_overlay_rgb_with_palette,
    state::{
        AudioPickerAction, OverlayHitContext, OverlayHitPoint, OverlayRenderContext, OverlayState,
        PlaylistMenuAction, SubtitlePickerAction, TransportControlAction,
    },
    style::OverlayPalette,
};

pub(crate) struct PlaybackOverlay {
    scratch: String,
    acrylic: AcrylicScratch,
    font: Option<FontRenderer>,
    palette: OverlayPalette,
}

impl PlaybackOverlay {
    pub(crate) fn new(fonts: &FontSystem, accent_color: [u8; 3]) -> Self {
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
            palette: OverlayPalette::new(accent_color),
        }
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        context: OverlayRenderContext,
        state: OverlayState,
    ) {
        render_overlay_rgb_with_palette(
            frame,
            context.width,
            context.height,
            context.terminal_cols,
            context.terminal_rows,
            context.scale_percent,
            state,
            self.palette,
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

    pub(crate) fn transport_control_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
    ) -> Option<TransportControlAction> {
        let metrics = self.metrics(context);
        transport_control_action(metrics, point)
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

    pub(crate) fn audio_picker_hover_index(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<usize> {
        let metrics = self.metrics(context);
        let row_count = labels.len();
        let visible_count = track_picker_visible_row_count(metrics, row_count);
        let scroll_offset = scroll_offset.min(row_count.saturating_sub(visible_count));
        let picker = picker_open.then(|| {
            track_picker_layout(metrics, labels, false, scroll_offset, self.font.as_mut())
        });
        track_picker_hover_index(
            metrics,
            point,
            picker,
            row_count,
            scroll_offset,
            visible_count,
        )
    }

    pub(crate) fn subtitle_picker_hover_index(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<usize> {
        let metrics = self.metrics(context);
        let row_count = labels.len().saturating_add(1);
        let visible_count = track_picker_visible_row_count(metrics, row_count);
        let scroll_offset = scroll_offset.min(row_count.saturating_sub(visible_count));
        let picker = picker_open
            .then(|| track_picker_layout(metrics, labels, true, scroll_offset, self.font.as_mut()));
        track_picker_hover_index(
            metrics,
            point,
            picker,
            row_count,
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

    pub(crate) fn playlist_menu_visible_row_count(
        &mut self,
        context: OverlayHitContext,
        labels: &[Arc<str>],
    ) -> usize {
        playlist_menu_visible_row_count(
            context.width,
            context.height,
            context.scale_percent,
            labels,
            self.font.as_mut(),
        )
    }

    pub(crate) fn playlist_menu_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<PlaylistMenuAction> {
        playlist_menu_action(
            context.width,
            context.height,
            context.scale_percent,
            point,
            scroll_offset,
            labels,
            self.font.as_mut(),
        )
    }

    pub(crate) fn playlist_menu_hover_index(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        scroll_offset: usize,
        labels: &[Arc<str>],
    ) -> Option<usize> {
        playlist_menu_hover_index(
            context.width,
            context.height,
            context.scale_percent,
            point,
            scroll_offset,
            labels,
            self.font.as_mut(),
        )
    }

    pub(crate) fn help_scroll_limit(&mut self, context: OverlayHitContext) -> usize {
        help_scroll_limit(
            context.width,
            context.height,
            context.scale_percent,
            self.font.as_mut(),
        )
    }

    fn metrics(&mut self, context: OverlayHitContext) -> OverlayMetrics {
        overlay_metrics(
            context.width,
            context.height,
            context.terminal_cols,
            context.terminal_rows,
            context.scale_percent,
            context.duration,
            context.playlist_previous_available,
            context.playlist_next_available,
            context.audio_available,
            context.subtitles_available,
            self.font.as_mut(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn render_overlay_rgb(
    frame: &mut [u8],
    width: u32,
    height: u32,
    terminal_cols: u16,
    terminal_rows: u16,
    scale_percent: u32,
    state: OverlayState,
    scratch: &mut String,
    acrylic: &mut AcrylicScratch,
    font: Option<&mut FontRenderer>,
) {
    render_overlay_rgb_with_palette(
        frame,
        width,
        height,
        terminal_cols,
        terminal_rows,
        scale_percent,
        state,
        OverlayPalette::new(crate::config::DEFAULT_ACCENT_COLOR),
        scratch,
        acrylic,
        font,
    );
}
