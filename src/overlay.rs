use std::time::Duration;

use crate::{
    font::FontRenderer,
    font_system::{FontRole, FontSystem},
};

const PANEL_COLOR: [u8; 3] = [18, 18, 22];
const TRACK_COLOR: [u8; 3] = [82, 82, 91];
const ACCENT_COLOR: [u8; 3] = [239, 68, 68];
const TEXT_COLOR: [u8; 3] = [250, 250, 250];
const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
const ACRYLIC_BLUR_RADIUS: u32 = 12;
const MIN_SCALE_PERCENT: u32 = 100;
const MAX_SCALE_PERCENT: u32 = 125;

#[derive(Clone)]
pub(crate) struct OverlayState {
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) paused: bool,
    pub(crate) visible: bool,
    pub(crate) audio_available: bool,
    pub(crate) selected_audio: Option<usize>,
    pub(crate) audio_picker_open: bool,
    pub(crate) audio_labels: Vec<&'static str>,
    pub(crate) subtitles_available: bool,
    pub(crate) selected_subtitle: Option<usize>,
    pub(crate) subtitle_picker_open: bool,
    pub(crate) subtitle_labels: Vec<&'static str>,
    pub(crate) status_message: Option<&'static str>,
    pub(crate) media_title: Option<&'static str>,
}

pub(crate) struct PlaybackOverlay {
    scratch: String,
    font: Option<FontRenderer>,
}

#[derive(Clone, Copy)]
pub(crate) struct OverlayHitContext {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_percent: u32,
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) audio_available: bool,
    pub(crate) audio_count: usize,
    pub(crate) subtitles_available: bool,
    pub(crate) subtitle_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AudioPickerAction {
    TogglePicker,
    SelectTrack(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubtitlePickerAction {
    TogglePicker,
    SelectOff,
    SelectTrack(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct OverlayHitPoint {
    pub(crate) x: u32,
    pub(crate) cell: HitboxRect,
}

#[derive(Clone, Copy)]
pub(crate) struct HitboxRect {
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
}

impl PlaybackOverlay {
    pub(crate) fn new(fonts: &FontSystem) -> Self {
        Self {
            scratch: String::new(),
            font: fonts
                .resolve_all(FontRole::Ui)
                .find_map(|path| FontRenderer::open_path(path, 18)),
        }
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        scale_percent: u32,
        state: OverlayState,
    ) {
        render_overlay_rgb(
            frame,
            width,
            height,
            scale_percent,
            state,
            &mut self.scratch,
            self.font.as_mut(),
        );
    }

    pub(crate) fn progress_hit_test(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
    ) -> Option<f64> {
        let metrics = self.metrics(
            context.width,
            context.height,
            context.scale_percent,
            context.duration,
            context.audio_available,
            context.subtitles_available,
        );
        progress_hit_ratio(metrics, point, context.position, context.duration)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn progress_ratio_from_x(
        &mut self,
        width: u32,
        height: u32,
        scale_percent: u32,
        duration: Option<Duration>,
        subtitles_available: bool,
        audio_available: bool,
        x: u32,
    ) -> f64 {
        let metrics = self.metrics(
            width,
            height,
            scale_percent,
            duration,
            audio_available,
            subtitles_available,
        );
        progress_ratio_for_x(metrics, x)
    }

    pub(crate) fn playback_button_hit_test(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
    ) -> bool {
        let metrics = self.metrics(
            context.width,
            context.height,
            context.scale_percent,
            context.duration,
            context.audio_available,
            context.subtitles_available,
        );
        playback_button_hit(metrics, point)
    }

    pub(crate) fn audio_picker_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
    ) -> Option<AudioPickerAction> {
        let metrics = self.metrics(
            context.width,
            context.height,
            context.scale_percent,
            context.duration,
            context.audio_available,
            context.subtitles_available,
        );
        audio_picker_action(metrics, point, picker_open, context.audio_count)
    }

    pub(crate) fn subtitle_picker_action(
        &mut self,
        context: OverlayHitContext,
        point: OverlayHitPoint,
        picker_open: bool,
    ) -> Option<SubtitlePickerAction> {
        let metrics = self.metrics(
            context.width,
            context.height,
            context.scale_percent,
            context.duration,
            context.audio_available,
            context.subtitles_available,
        );
        subtitle_picker_action(metrics, point, picker_open, context.subtitle_count)
    }

    fn metrics(
        &mut self,
        width: u32,
        height: u32,
        scale_percent: u32,
        duration: Option<Duration>,
        audio_available: bool,
        subtitles_available: bool,
    ) -> OverlayMetrics {
        overlay_metrics(
            width,
            height,
            scale_percent,
            duration,
            audio_available,
            subtitles_available,
            self.font.as_mut(),
        )
    }
}

#[derive(Clone, Copy)]
struct OverlayMetrics {
    panel_y: u32,
    panel_height: u32,
    inset_x: u32,
    inner_x: u32,
    text_y: u32,
    bar_x: u32,
    bar_y: u32,
    bar_width: u32,
    bar_height: u32,
    control_size: u32,
    control_y: u32,
    audio_x: u32,
    subtitle_x: u32,
    time_x: u32,
    text_size: u32,
    fallback_text_scale: u32,
    panel_right: u32,
}

impl OverlayMetrics {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: u32,
        video_height: u32,
        text_size: u32,
        fallback_text_scale: u32,
        text_height: u32,
        time_width: u32,
        audio_available: bool,
        subtitles_available: bool,
    ) -> Self {
        let bar_height = bar_height_for_text(text_size).min(video_height.max(1));
        let vertical_pad = vertical_padding_for_text(text_size);
        let outer_y = outer_padding_for_text(text_size);
        let control_size = control_size_for_text(text_size, text_height);
        let control_gap = control_gap_for_text(text_size);
        let handle_radius = progress_handle_radius(bar_height);
        let row_height = text_height
            .max(control_size)
            .max(handle_radius.saturating_mul(2).saturating_add(4));
        let panel_height = vertical_pad
            .saturating_add(row_height)
            .saturating_add(vertical_pad)
            .max(1);
        let height = panel_height
            .saturating_add(outer_y.saturating_mul(2))
            .min(video_height.max(1));
        let top = video_height.saturating_sub(height);
        let panel_y = top.saturating_add(outer_y.min(height.saturating_sub(1) / 2));
        let panel_height = panel_height.min(height.saturating_sub(outer_y).max(1));
        let inset_x = (width / 48).clamp(8, 34).min(width.saturating_sub(1) / 2);
        let panel_right = width.saturating_sub(inset_x);
        let inner_pad = horizontal_padding_for_text(text_size);
        let inner_x = inset_x
            .saturating_add(inner_pad)
            .min(width.saturating_sub(1));
        let row_y = panel_y.saturating_add(vertical_pad);
        let control_y = row_y.saturating_add((row_height.saturating_sub(control_size)) / 2);
        let text_y = row_y.saturating_add((row_height.saturating_sub(text_height)) / 2);
        let time_x = inner_x
            .saturating_add(control_size)
            .saturating_add(control_gap)
            .min(width.saturating_sub(1));
        let content_right = width.saturating_sub(inner_x).max(inner_x.saturating_add(1));
        let controls = u32::from(audio_available).saturating_add(u32::from(subtitles_available));
        let controls_width = controls
            .saturating_mul(control_size)
            .saturating_add(controls.saturating_sub(1).saturating_mul(control_gap));
        let controls_left = content_right.saturating_sub(controls_width);
        let mut next_control_x = controls_left;
        let audio_x = if audio_available {
            let x = next_control_x;
            next_control_x = next_control_x
                .saturating_add(control_size)
                .saturating_add(control_gap);
            x
        } else {
            content_right
        };
        let subtitle_x = if subtitles_available {
            next_control_x
        } else {
            content_right
        };
        let bar_gap = control_gap.saturating_mul(3);
        let bar_x = time_x
            .saturating_add(time_width)
            .saturating_add(bar_gap)
            .min(controls_left.saturating_sub(1));
        let bar_right = if controls > 0 {
            controls_left.saturating_sub(bar_gap)
        } else {
            content_right.saturating_sub(bar_gap)
        };
        let bar_width = bar_right.saturating_sub(bar_x).max(1);
        let bar_y = row_y.saturating_add((row_height.saturating_sub(bar_height)) / 2);

        Self {
            panel_y,
            panel_height,
            inset_x,
            inner_x,
            text_y,
            bar_x,
            bar_y,
            bar_width,
            bar_height,
            control_size,
            control_y,
            audio_x,
            subtitle_x,
            time_x,
            text_size,
            fallback_text_scale,
            panel_right,
        }
    }
}

fn render_overlay_rgb(
    frame: &mut [u8],
    width: u32,
    height: u32,
    scale_percent: u32,
    state: OverlayState,
    scratch: &mut String,
    font: Option<&mut FontRenderer>,
) {
    if width == 0 || height == 0 || frame.len() < (width as usize * height as usize * 3) {
        return;
    }
    if !state.visible && state.status_message.is_none() {
        return;
    }

    let text_size = text_size(width, height, scale_percent);
    let fallback_text_scale = fallback_text_scale(width, height, scale_percent);
    let mut font = font.and_then(|font| font.set_pixel_size(text_size).then_some(font));
    let text_height = font
        .as_ref()
        .map(|font| font.line_height())
        .unwrap_or(7 * fallback_text_scale);

    if let Some(message) = state.status_message {
        draw_top_message(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            message,
            HorizontalAnchor::Right,
        );
    }

    if !state.visible {
        return;
    }

    if let Some(title) = state.media_title {
        draw_top_message(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            title,
            HorizontalAnchor::Left,
        );
    }

    let time_width = time_column_width(font.as_deref_mut(), state.duration, fallback_text_scale);
    let metrics = OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        time_width,
        state.audio_available,
        state.subtitles_available,
    );
    let panel_width = width
        .saturating_sub(metrics.inset_x.saturating_mul(2))
        .max(1);
    let panel_radius = rounded_radius(panel_width, metrics.panel_height, metrics.text_size);
    let panel_rect = RoundedRect {
        x: f64::from(metrics.inset_x),
        y: f64::from(metrics.panel_y),
        width: f64::from(panel_width),
        height: f64::from(metrics.panel_height),
        radius: f64::from(panel_radius),
    };
    fill_acrylic_rounded_rect(frame, width, height, panel_rect, PANEL_COLOR, 188);

    let bar_radius = rounded_radius(
        metrics.bar_width,
        metrics.bar_height,
        metrics.bar_height / 2,
    );
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(metrics.bar_x),
            y: f64::from(metrics.bar_y),
            width: f64::from(metrics.bar_width),
            height: f64::from(metrics.bar_height),
            radius: f64::from(bar_radius),
        },
        TRACK_COLOR,
        218,
    );

    let filled = progress_pixels(metrics.bar_width, state.position, state.duration);
    if filled > 0 {
        fill_rounded_rect(
            frame,
            width,
            height,
            RoundedRect {
                x: f64::from(metrics.bar_x),
                y: f64::from(metrics.bar_y),
                width: f64::from(filled),
                height: f64::from(metrics.bar_height),
                radius: f64::from(rounded_radius(filled, metrics.bar_height, bar_radius)),
            },
            ACCENT_COLOR,
            248,
        );
    }
    if state.duration.is_some_and(|duration| !duration.is_zero()) {
        draw_progress_handle(frame, width, height, metrics, filled);
    }

    scratch.clear();
    scratch.push_str(&format_position_timestamp(state.position, state.duration));
    scratch.push_str(" / ");
    if let Some(duration) = state.duration {
        scratch.push_str(&format_timestamp(duration));
    } else {
        scratch.push_str("--:--");
    }

    draw_playback_control(frame, width, height, metrics, state.paused);
    if state.audio_available {
        draw_audio_control(
            frame,
            width,
            height,
            metrics,
            state.selected_audio.is_some(),
        );
        if state.audio_picker_open {
            draw_track_picker(
                font.as_deref_mut(),
                frame,
                width,
                height,
                metrics,
                track_picker_anchor_x(metrics),
                &state.audio_labels,
                state.selected_audio,
                false,
            );
        }
    }
    if state.subtitles_available {
        draw_subtitle_control(
            frame,
            width,
            height,
            metrics,
            state.selected_subtitle.is_some(),
        );
        if state.subtitle_picker_open {
            draw_track_picker(
                font.as_deref_mut(),
                frame,
                width,
                height,
                metrics,
                track_picker_anchor_x(metrics),
                &state.subtitle_labels,
                state.selected_subtitle,
                true,
            );
        }
    }
    draw_overlay_text(
        font,
        frame,
        width,
        height,
        metrics.time_x,
        metrics.text_y,
        metrics.fallback_text_scale,
        scratch,
        TEXT_COLOR,
        238,
    );
}

#[derive(Clone, Copy)]
enum HorizontalAnchor {
    Left,
    Right,
}

#[allow(clippy::too_many_arguments)]
fn draw_top_message(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    text: &str,
    anchor: HorizontalAnchor,
) {
    let inset_x = (width / 48).clamp(8, 34).min(width.saturating_sub(1));
    let inset_y = top_message_y(height, text_size);
    let pad_x = (horizontal_padding_for_text(text_size) / 2).max(6);
    let pad_y = (vertical_padding_for_text(text_size) / 2).max(4);
    let text_width = font
        .as_mut()
        .map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, fallback_scale));
    let panel_width = text_width
        .saturating_add(pad_x.saturating_mul(2))
        .min(width.saturating_sub(inset_x).max(1));
    let panel_height = text_height
        .saturating_add(pad_y.saturating_mul(2))
        .min(height.saturating_sub(inset_y).max(1));
    let panel_x = match anchor {
        HorizontalAnchor::Left => inset_x,
        HorizontalAnchor::Right => width
            .saturating_sub(inset_x)
            .saturating_sub(panel_width)
            .max(inset_x.min(width.saturating_sub(1))),
    };
    let panel_radius = rounded_radius(panel_width, panel_height, text_size / 3);

    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(panel_x),
            y: f64::from(inset_y),
            width: f64::from(panel_width),
            height: f64::from(panel_height),
            radius: f64::from(panel_radius),
        },
        PANEL_COLOR,
        202,
    );

    draw_overlay_text(
        font,
        frame,
        width,
        height,
        panel_x.saturating_add(pad_x).min(width.saturating_sub(1)),
        inset_y.saturating_add(pad_y).min(height.saturating_sub(1)),
        fallback_scale,
        text,
        TEXT_COLOR,
        244,
    );
}

fn top_message_y(height: u32, text_size: u32) -> u32 {
    outer_padding_for_text(text_size).min(height.saturating_sub(1))
}

fn overlay_metrics(
    width: u32,
    height: u32,
    scale_percent: u32,
    duration: Option<Duration>,
    audio_available: bool,
    subtitles_available: bool,
    font: Option<&mut FontRenderer>,
) -> OverlayMetrics {
    let text_size = text_size(width, height, scale_percent);
    let fallback_text_scale = fallback_text_scale(width, height, scale_percent);
    let mut font = font;
    let text_height = font
        .as_mut()
        .and_then(|font| font.set_pixel_size(text_size).then(|| font.line_height()))
        .unwrap_or(7 * fallback_text_scale);
    let time_width = time_column_width(font, duration, fallback_text_scale);
    OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        time_width,
        audio_available,
        subtitles_available,
    )
}

fn draw_playback_control(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    paused: bool,
) {
    let size = metrics.control_size;
    if size == 0 {
        return;
    }

    let x = metrics.inner_x;
    let y = metrics.control_y;
    if paused {
        draw_play_icon(frame, width, height, x, y, size);
    } else {
        draw_pause_icon(frame, width, height, x, y, size);
    }
}

fn draw_play_icon(frame: &mut [u8], width: u32, height: u32, x: u32, y: u32, size: u32) {
    let x = f64::from(x);
    let y = f64::from(y);
    let size = f64::from(size);
    fill_triangle(
        frame,
        width,
        height,
        Triangle {
            a: Point {
                x: x + size * 0.30,
                y: y + size * 0.21,
            },
            b: Point {
                x: x + size * 0.30,
                y: y + size * 0.79,
            },
            c: Point {
                x: x + size * 0.77,
                y: y + size * 0.50,
            },
        },
        TEXT_COLOR,
        245,
    );
}

fn draw_pause_icon(frame: &mut [u8], width: u32, height: u32, x: u32, y: u32, size: u32) {
    let bar_width = (size / 5).max(2);
    let bar_height = (size * 3 / 5).max(5);
    let gap = (size / 7).max(2);
    let total_width = bar_width.saturating_mul(2).saturating_add(gap);
    let start_x = x + size.saturating_sub(total_width) / 2;
    let start_y = y + size.saturating_sub(bar_height) / 2;
    let radius = rounded_radius(bar_width, bar_height, bar_width / 2);

    for bar_x in [start_x, start_x + bar_width + gap] {
        fill_rounded_rect(
            frame,
            width,
            height,
            RoundedRect {
                x: f64::from(bar_x),
                y: f64::from(start_y),
                width: f64::from(bar_width),
                height: f64::from(bar_height),
                radius: f64::from(radius),
            },
            TEXT_COLOR,
            245,
        );
    }
}

fn draw_audio_control(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    active: bool,
) {
    draw_audio_icon(
        frame,
        width,
        height,
        metrics,
        if active { 238 } else { 132 },
    );
}

fn draw_audio_icon(frame: &mut [u8], width: u32, height: u32, metrics: OverlayMetrics, alpha: u8) {
    let icon_width = (metrics.control_size * 4 / 5).max(14);
    let icon_height = (metrics.control_size * 3 / 5).max(10);
    let icon_x = metrics
        .audio_x
        .saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let icon_y = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);
    let radius = rounded_radius(icon_width, icon_height, icon_height / 4);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(icon_x),
            y: f64::from(icon_y),
            width: f64::from(icon_width),
            height: f64::from(icon_height),
            radius: f64::from(radius),
        },
        TEXT_COLOR,
        alpha,
    );

    let stroke = (icon_height / 8).max(2);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(icon_x.saturating_add(stroke)),
            y: f64::from(icon_y.saturating_add(stroke)),
            width: f64::from(icon_width.saturating_sub(stroke.saturating_mul(2))),
            height: f64::from(icon_height.saturating_sub(stroke.saturating_mul(2))),
            radius: f64::from(radius.saturating_sub(stroke / 2)),
        },
        PANEL_COLOR,
        255,
    );

    let bar_w = stroke.max(2);
    let gap = stroke.max(2);
    let total_w = bar_w
        .saturating_mul(3)
        .saturating_add(gap.saturating_mul(2));
    let start_x = icon_x.saturating_add(icon_width.saturating_sub(total_w) / 2);
    let base_y = icon_y
        .saturating_add(icon_height)
        .saturating_sub(stroke.saturating_mul(2));
    let bar_heights = [
        icon_height.saturating_mul(3) / 10,
        icon_height.saturating_mul(5) / 10,
        icon_height.saturating_mul(4) / 10,
    ];
    for (index, bar_h) in bar_heights.into_iter().enumerate() {
        let bar_x = start_x.saturating_add((bar_w + gap).saturating_mul(index as u32));
        let bar_y = base_y.saturating_sub(bar_h);
        fill_rounded_rect(
            frame,
            width,
            height,
            RoundedRect {
                x: f64::from(bar_x),
                y: f64::from(bar_y),
                width: f64::from(bar_w),
                height: f64::from(bar_h),
                radius: f64::from(bar_w),
            },
            TEXT_COLOR,
            alpha,
        );
    }
}

fn draw_subtitle_control(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    visible: bool,
) {
    draw_subtitle_icon(
        frame,
        width,
        height,
        metrics,
        if visible { 238 } else { 132 },
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_track_picker(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    anchor_x: u32,
    labels: &[&str],
    selected_track: Option<usize>,
    include_off: bool,
) {
    let max_label_width = labels
        .iter()
        .copied()
        .chain(include_off.then_some("Off"))
        .map(|label| picker_text_width(font.as_deref_mut(), label, metrics.fallback_text_scale))
        .max()
        .unwrap_or(0);
    let picker_width = track_picker_width(metrics, anchor_x, max_label_width);
    let picker = track_picker_rect(metrics, anchor_x, labels.len(), picker_width, include_off);
    let radius = rounded_radius(
        picker.right.saturating_sub(picker.left),
        picker.bottom.saturating_sub(picker.top),
        metrics.text_size / 3,
    );
    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(picker.left),
            y: f64::from(picker.top),
            width: f64::from(picker.right.saturating_sub(picker.left)),
            height: f64::from(picker.bottom.saturating_sub(picker.top)),
            radius: f64::from(radius),
        },
        PANEL_COLOR,
        188,
    );

    let pad = picker_padding(metrics);
    let row_height = track_picker_row_height(metrics);
    let marker_size = (metrics.text_size / 3).clamp(4, 7);
    let marker_x = picker.left.saturating_add(pad);
    let text_x = marker_x.saturating_add(marker_size).saturating_add(pad / 2);
    let text_width = picker.right.saturating_sub(text_x).saturating_sub(pad);
    for (index, label) in labels.iter().enumerate() {
        let row_y = picker
            .top
            .saturating_add(pad)
            .saturating_add(row_height.saturating_mul(index as u32));
        if selected_track == Some(index) {
            draw_picker_marker(
                frame,
                width,
                height,
                marker_x,
                row_y,
                row_height,
                marker_size,
            );
        }
        let label = fit_picker_text(
            font.as_deref_mut(),
            label,
            metrics.fallback_text_scale,
            text_width,
        );
        draw_overlay_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_x,
            row_y,
            metrics.fallback_text_scale,
            &label,
            TEXT_COLOR,
            244,
        );
    }

    if include_off {
        let off_y = picker
            .top
            .saturating_add(pad)
            .saturating_add(row_height.saturating_mul(labels.len() as u32));
        if selected_track.is_none() {
            draw_picker_marker(
                frame,
                width,
                height,
                marker_x,
                off_y,
                row_height,
                marker_size,
            );
        }
        draw_overlay_text(
            font,
            frame,
            width,
            height,
            text_x,
            off_y,
            metrics.fallback_text_scale,
            "Off",
            TEXT_COLOR,
            210,
        );
    }
}

fn draw_subtitle_icon(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    alpha: u8,
) {
    let icon_width = (metrics.control_size * 4 / 5).max(14);
    let icon_height = (metrics.control_size * 3 / 5).max(10);
    let icon_x = metrics
        .subtitle_x
        .saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let icon_y = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);
    let radius = rounded_radius(icon_width, icon_height, icon_height / 4);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(icon_x),
            y: f64::from(icon_y),
            width: f64::from(icon_width),
            height: f64::from(icon_height),
            radius: f64::from(radius),
        },
        TEXT_COLOR,
        alpha,
    );

    let stroke = (icon_height / 8).max(2);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(icon_x.saturating_add(stroke)),
            y: f64::from(icon_y.saturating_add(stroke)),
            width: f64::from(icon_width.saturating_sub(stroke.saturating_mul(2))),
            height: f64::from(icon_height.saturating_sub(stroke.saturating_mul(2))),
            radius: f64::from(radius.saturating_sub(stroke / 2)),
        },
        PANEL_COLOR,
        255,
    );

    let line_height = stroke.max(2);
    let short_width = icon_width / 3;
    let long_width = icon_width / 2;
    let top_y = icon_y
        .saturating_add(icon_height / 2)
        .saturating_sub(line_height);
    let bottom_y = top_y.saturating_add(line_height.saturating_mul(2));
    let left_x = icon_x.saturating_add(icon_width / 4);
    let right_x = icon_x.saturating_add(icon_width / 2);
    for (line_x, line_y, line_width) in [
        (right_x, top_y, short_width),
        (left_x, bottom_y, long_width),
    ] {
        fill_rounded_rect(
            frame,
            width,
            height,
            RoundedRect {
                x: f64::from(line_x),
                y: f64::from(line_y),
                width: f64::from(line_width),
                height: f64::from(line_height),
                radius: f64::from(line_height),
            },
            TEXT_COLOR,
            alpha,
        );
    }
}

fn picker_text_width(font: Option<&mut FontRenderer>, text: &str, fallback_scale: u32) -> u32 {
    font.map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, fallback_scale))
}

fn fit_picker_text(
    mut font: Option<&mut FontRenderer>,
    text: &str,
    fallback_scale: u32,
    max_width: u32,
) -> String {
    if picker_text_width(font.as_deref_mut(), text, fallback_scale) <= max_width {
        return text.to_string();
    }

    let suffix = "...";
    let suffix_width = picker_text_width(font.as_deref_mut(), suffix, fallback_scale);
    if suffix_width > max_width {
        return String::new();
    }

    let mut trimmed = text.to_string();
    while !trimmed.is_empty() {
        trimmed.pop();
        let candidate = format!("{trimmed}{suffix}");
        if picker_text_width(font.as_deref_mut(), &candidate, fallback_scale) <= max_width {
            return candidate;
        }
    }
    suffix.to_string()
}

fn draw_picker_marker(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    row_y: u32,
    row_height: u32,
    size: u32,
) {
    fill_circle(
        frame,
        width,
        height,
        Circle {
            x: f64::from(x.saturating_add(size / 2)),
            y: f64::from(row_y.saturating_add(row_height / 2)),
            radius: f64::from(size / 2),
        },
        ACCENT_COLOR,
        245,
    );
}

fn playback_button_hit(metrics: OverlayMetrics, point: OverlayHitPoint) -> bool {
    hitbox_intersects(
        point.cell,
        HitboxRect {
            left: metrics.inner_x,
            top: metrics.control_y,
            right: metrics.inner_x.saturating_add(metrics.control_size),
            bottom: metrics.control_y.saturating_add(metrics.control_size),
        },
    )
}

fn audio_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker_open: bool,
    audio_count: usize,
) -> Option<AudioPickerAction> {
    if picker_open {
        let anchor_x = track_picker_anchor_x(metrics);
        let picker = track_picker_rect(
            metrics,
            anchor_x,
            audio_count,
            track_picker_max_width(metrics, anchor_x),
            false,
        );
        for index in 0..audio_count {
            if hitbox_intersects(point.cell, track_picker_track_rect(metrics, picker, index)) {
                return Some(AudioPickerAction::SelectTrack(index));
            }
        }
    }

    hitbox_intersects(point.cell, audio_button_rect(metrics))
        .then_some(AudioPickerAction::TogglePicker)
}

fn subtitle_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker_open: bool,
    subtitle_count: usize,
) -> Option<SubtitlePickerAction> {
    if picker_open {
        let anchor_x = track_picker_anchor_x(metrics);
        let picker = track_picker_rect(
            metrics,
            anchor_x,
            subtitle_count,
            track_picker_max_width(metrics, anchor_x),
            true,
        );
        for index in 0..subtitle_count {
            if hitbox_intersects(point.cell, track_picker_track_rect(metrics, picker, index)) {
                return Some(SubtitlePickerAction::SelectTrack(index));
            }
        }
        if hitbox_intersects(
            point.cell,
            track_picker_off_rect(metrics, picker, subtitle_count),
        ) {
            return Some(SubtitlePickerAction::SelectOff);
        }
    }

    hitbox_intersects(point.cell, subtitle_button_rect(metrics))
        .then_some(SubtitlePickerAction::TogglePicker)
}

fn audio_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    icon_button_rect(metrics.audio_x, metrics)
}

fn subtitle_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    icon_button_rect(metrics.subtitle_x, metrics)
}

fn icon_button_rect(x: u32, metrics: OverlayMetrics) -> HitboxRect {
    let icon_width = (metrics.control_size * 4 / 5).max(14);
    let icon_height = (metrics.control_size * 3 / 5).max(10);
    let left = x.saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let top = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);

    HitboxRect {
        left,
        top,
        right: left.saturating_add(icon_width),
        bottom: top.saturating_add(icon_height),
    }
}

fn track_picker_anchor_x(metrics: OverlayMetrics) -> u32 {
    metrics.panel_right.saturating_sub(metrics.control_size)
}

fn track_picker_rect(
    metrics: OverlayMetrics,
    anchor_x: u32,
    track_count: usize,
    picker_width: u32,
    include_off: bool,
) -> HitboxRect {
    let pad = picker_padding(metrics);
    let row_height = track_picker_row_height(metrics);
    let row_count = track_count.saturating_add(usize::from(include_off));
    let picker_height = row_height
        .saturating_mul(row_count as u32)
        .saturating_add(pad.saturating_mul(2));
    let right = anchor_x
        .saturating_add(metrics.control_size)
        .max(picker_width);
    let left = right.saturating_sub(picker_width);
    let bottom = metrics
        .panel_y
        .saturating_sub(track_picker_gap_for_text(metrics.text_size));
    let top = bottom.saturating_sub(picker_height);
    HitboxRect {
        left,
        top,
        right,
        bottom,
    }
}

fn track_picker_width(metrics: OverlayMetrics, anchor_x: u32, label_width: u32) -> u32 {
    let pad = picker_padding(metrics);
    let marker_size = (metrics.text_size / 3).clamp(4, 7);
    let desired = pad
        .saturating_mul(2)
        .saturating_add(marker_size)
        .saturating_add(pad / 2)
        .saturating_add(label_width)
        .max(scaled_normal_pixels(132, metrics.text_size))
        .max(metrics.control_size);
    desired.min(track_picker_max_width(metrics, anchor_x).max(1))
}

fn track_picker_max_width(metrics: OverlayMetrics, anchor_x: u32) -> u32 {
    anchor_x
        .saturating_add(metrics.control_size)
        .saturating_sub(metrics.inset_x)
        .max(metrics.control_size)
}

fn track_picker_track_rect(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    index: usize,
) -> HitboxRect {
    let pad = picker_padding(metrics);
    let row_height = track_picker_row_height(metrics);
    let top = picker
        .top
        .saturating_add(pad)
        .saturating_add(row_height.saturating_mul(index as u32));
    HitboxRect {
        left: picker.left,
        top,
        right: picker.right,
        bottom: top.saturating_add(row_height),
    }
}

fn track_picker_off_rect(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    track_count: usize,
) -> HitboxRect {
    track_picker_track_rect(metrics, picker, track_count)
}

fn picker_padding(metrics: OverlayMetrics) -> u32 {
    (horizontal_padding_for_text(metrics.text_size) / 2).max(6)
}

fn track_picker_row_height(metrics: OverlayMetrics) -> u32 {
    metrics
        .text_size
        .max(7 * metrics.fallback_text_scale)
        .saturating_add(6)
}

fn draw_progress_handle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    filled: u32,
) {
    let radius = progress_handle_radius(metrics.bar_height);
    let center_x = f64::from(metrics.bar_x + filled.min(metrics.bar_width));
    let center_y = f64::from(metrics.bar_y) + f64::from(metrics.bar_height) / 2.0;

    fill_circle(
        frame,
        width,
        height,
        Circle {
            x: center_x,
            y: center_y + 0.75,
            radius: f64::from(radius + 2),
        },
        SHADOW_COLOR,
        112,
    );
    fill_circle(
        frame,
        width,
        height,
        Circle {
            x: center_x,
            y: center_y,
            radius: f64::from(radius),
        },
        ACCENT_COLOR,
        255,
    );
}

fn progress_hit_ratio(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    position: Duration,
    duration: Option<Duration>,
) -> Option<f64> {
    let hit_radius = progress_handle_radius(metrics.bar_height).max(8) + 5;
    let center_y = progress_handle_center_y(metrics);
    let bar_rect = HitboxRect {
        left: metrics.bar_x,
        top: center_y.saturating_sub(hit_radius),
        right: metrics.bar_x.saturating_add(metrics.bar_width),
        bottom: center_y.saturating_add(hit_radius),
    };
    let filled = progress_pixels(metrics.bar_width, position, duration);
    let handle_center_x = metrics.bar_x.saturating_add(filled.min(metrics.bar_width));
    let handle_rect = HitboxRect {
        left: handle_center_x.saturating_sub(hit_radius),
        top: center_y.saturating_sub(hit_radius),
        right: handle_center_x.saturating_add(hit_radius),
        bottom: center_y.saturating_add(hit_radius),
    };
    if !hitbox_intersects(point.cell, bar_rect) && !hitbox_intersects(point.cell, handle_rect) {
        return None;
    }

    Some(progress_ratio_for_x(metrics, point.x))
}

fn progress_handle_center_y(metrics: OverlayMetrics) -> u32 {
    metrics.bar_y.saturating_add(metrics.bar_height / 2)
}

fn hitbox_intersects(a: HitboxRect, b: HitboxRect) -> bool {
    a.left <= b.right && a.right >= b.left && a.top <= b.bottom && a.bottom >= b.top
}

fn progress_ratio_for_x(metrics: OverlayMetrics, x: u32) -> f64 {
    let end_x = metrics.bar_x.saturating_add(metrics.bar_width);
    let x = x.clamp(metrics.bar_x, end_x);
    f64::from(x.saturating_sub(metrics.bar_x)) / f64::from(metrics.bar_width.max(1))
}

fn text_size(width: u32, video_height: u32, scale_percent: u32) -> u32 {
    let base = if width >= 420 && video_height >= 240 {
        18
    } else {
        12
    };
    scaled_overlay_pixels(base, scale_percent)
}

fn fallback_text_scale(width: u32, video_height: u32, scale_percent: u32) -> u32 {
    (text_size(width, video_height, scale_percent) / 7).clamp(1, 4)
}

fn scaled_overlay_pixels(value: u32, scale_percent: u32) -> u32 {
    let scale_percent = scale_percent.clamp(MIN_SCALE_PERCENT, MAX_SCALE_PERCENT);
    (value.saturating_mul(scale_percent).saturating_add(50) / 100).max(1)
}

fn scaled_normal_pixels(value: u32, text_size: u32) -> u32 {
    if text_size >= 18 {
        value.saturating_mul(text_size).saturating_add(9) / 18
    } else {
        value
    }
}

fn bar_height_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(6, text_size),
        _ => 5,
    }
}

fn vertical_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(11, text_size),
        _ => 8,
    }
}

fn horizontal_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(18, text_size),
        _ => 12,
    }
}

fn control_size_for_text(text_size: u32, text_height: u32) -> u32 {
    text_height.max(text_size).max(12)
}

fn control_gap_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(10, text_size),
        _ => 8,
    }
}

fn track_picker_gap_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(8, text_size),
        _ => 6,
    }
}

fn time_column_width(
    font: Option<&mut FontRenderer>,
    duration: Option<Duration>,
    fallback_scale: u32,
) -> u32 {
    let text = time_column_template(duration);
    font.map(|font| font.text_width(&text))
        .unwrap_or_else(|| bitmap_text_width(&text, fallback_scale))
}

fn time_column_template(duration: Option<Duration>) -> String {
    if let Some(duration) = duration {
        let position = format_position_timestamp(Duration::ZERO, Some(duration));
        let duration = format_timestamp(duration);
        format!("{position} / {duration}")
    } else {
        "0:00 / --:--".to_string()
    }
}

fn format_position_timestamp(position: Duration, duration: Option<Duration>) -> String {
    let Some(duration) = duration.filter(|duration| duration.as_secs() >= 3600) else {
        return format_timestamp(position);
    };

    format_timestamp_with_hours(position, hour_digits(duration))
}

fn hour_digits(duration: Duration) -> usize {
    ((duration.as_secs() / 3600).max(1)).to_string().len()
}

fn outer_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(6, text_size),
        _ => 4,
    }
}

fn progress_handle_radius(bar_height: u32) -> u32 {
    (bar_height * 7 / 5).clamp(6, 14)
}

fn rounded_radius(width: u32, height: u32, wanted: u32) -> u32 {
    wanted.max(1).min(width.max(1) / 2).min(height.max(1) / 2)
}

fn progress_pixels(width: u32, position: Duration, duration: Option<Duration>) -> u32 {
    let Some(duration) = duration.filter(|duration| !duration.is_zero()) else {
        return 0;
    };
    let ratio = (position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0);
    (ratio * f64::from(width)).round() as u32
}

fn format_timestamp(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;

    if hours > 0 {
        format_timestamp_with_hours(duration, hour_digits(duration))
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_timestamp_with_hours(duration: Duration, hour_width: usize) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;

    format!("{hours:0hour_width$}:{minutes:02}:{seconds:02}")
}

#[derive(Clone, Copy)]
struct RoundedRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
}

#[derive(Clone, Copy)]
struct Circle {
    x: f64,
    y: f64,
    radius: f64,
}

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
struct Triangle {
    a: Point,
    b: Point,
    c: Point,
}

fn fill_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    let min_x = rect.x.floor().max(0.0) as u32;
    let max_x = (rect.x + rect.width).ceil().min(f64::from(width)) as u32;
    let min_y = rect.y.floor().max(0.0) as u32;
    let max_y = (rect.y + rect.height).ceil().min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let coverage = rounded_rect_coverage(f64::from(x) + 0.5, f64::from(y) + 0.5, rect);
            if coverage > 0.0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(
                    frame,
                    offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
}

fn fill_acrylic_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    color: [u8; 3],
    alpha: u8,
) {
    blur_rounded_rect(frame, width, height, rect, ACRYLIC_BLUR_RADIUS);
    fill_rounded_rect(frame, width, height, rect, color, alpha);
}

fn blur_rounded_rect(frame: &mut [u8], width: u32, height: u32, rect: RoundedRect, radius: u32) {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 || radius == 0 {
        return;
    }

    let min_x = rect.x.floor().max(0.0) as u32;
    let max_x = (rect.x + rect.width).ceil().min(f64::from(width)) as u32;
    let min_y = rect.y.floor().max(0.0) as u32;
    let max_y = (rect.y + rect.height).ceil().min(f64::from(height)) as u32;
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let sample_left = min_x.saturating_sub(radius);
    let sample_top = min_y.saturating_sub(radius);
    let sample_right = max_x.saturating_add(radius).min(width);
    let sample_bottom = max_y.saturating_add(radius).min(height);
    let sample_width = sample_right.saturating_sub(sample_left);
    let sample_height = sample_bottom.saturating_sub(sample_top);
    if sample_width == 0 || sample_height == 0 {
        return;
    }

    let sample_len = (sample_width as usize)
        .saturating_mul(sample_height as usize)
        .saturating_mul(3);
    let mut source = vec![0_u8; sample_len];
    for y in 0..sample_height {
        let source_start = rgb_offset(width, sample_left, sample_top + y);
        let source_end = source_start + sample_width as usize * 3;
        let target_start = (y * sample_width * 3) as usize;
        let target_end = target_start + sample_width as usize * 3;
        source[target_start..target_end].copy_from_slice(&frame[source_start..source_end]);
    }

    let mut horizontal = vec![0_u8; source.len()];
    let mut blurred = vec![0_u8; source.len()];
    horizontal_box_blur_rgb(
        &source,
        &mut horizontal,
        sample_width,
        sample_height,
        radius,
    );
    vertical_box_blur_rgb(
        &horizontal,
        &mut blurred,
        sample_width,
        sample_height,
        radius,
    );

    for y in min_y..max_y {
        for x in min_x..max_x {
            let coverage = rounded_rect_coverage(f64::from(x) + 0.5, f64::from(y) + 0.5, rect);
            if coverage <= 0.0 {
                continue;
            }

            let source_offset = rgb_offset(sample_width, x - sample_left, y - sample_top);
            let target_offset = rgb_offset(width, x, y);
            let blurred_pixel = [
                blurred[source_offset],
                blurred[source_offset + 1],
                blurred[source_offset + 2],
            ];
            blend_pixel(
                frame,
                target_offset,
                blurred_pixel,
                (coverage * 255.0).round() as u8,
            );
        }
    }
}

fn horizontal_box_blur_rgb(source: &[u8], target: &mut [u8], width: u32, height: u32, radius: u32) {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    if width == 0 || height == 0 {
        return;
    }

    for y in 0..height {
        let row_start = y * width * 3;
        let mut left = 0_usize;
        let mut right = 0_usize;
        let mut sum = [0_u32; 3];

        for x in 0..width {
            let wanted_left = x.saturating_sub(radius);
            let wanted_right = (x + radius).min(width - 1);

            while right <= wanted_right {
                let offset = row_start + right * 3;
                sum[0] += u32::from(source[offset]);
                sum[1] += u32::from(source[offset + 1]);
                sum[2] += u32::from(source[offset + 2]);
                right += 1;
            }

            while left < wanted_left {
                let offset = row_start + left * 3;
                sum[0] -= u32::from(source[offset]);
                sum[1] -= u32::from(source[offset + 1]);
                sum[2] -= u32::from(source[offset + 2]);
                left += 1;
            }

            let count = (right - left) as u32;
            let target_offset = row_start + x * 3;
            target[target_offset] = (sum[0] / count) as u8;
            target[target_offset + 1] = (sum[1] / count) as u8;
            target[target_offset + 2] = (sum[2] / count) as u8;
        }
    }
}

fn vertical_box_blur_rgb(source: &[u8], target: &mut [u8], width: u32, height: u32, radius: u32) {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    if width == 0 || height == 0 {
        return;
    }

    for x in 0..width {
        let mut top = 0_usize;
        let mut bottom = 0_usize;
        let mut sum = [0_u32; 3];

        for y in 0..height {
            let wanted_top = y.saturating_sub(radius);
            let wanted_bottom = (y + radius).min(height - 1);

            while bottom <= wanted_bottom {
                let offset = (bottom * width + x) * 3;
                sum[0] += u32::from(source[offset]);
                sum[1] += u32::from(source[offset + 1]);
                sum[2] += u32::from(source[offset + 2]);
                bottom += 1;
            }

            while top < wanted_top {
                let offset = (top * width + x) * 3;
                sum[0] -= u32::from(source[offset]);
                sum[1] -= u32::from(source[offset + 1]);
                sum[2] -= u32::from(source[offset + 2]);
                top += 1;
            }

            let count = (bottom - top) as u32;
            let target_offset = (y * width + x) * 3;
            target[target_offset] = (sum[0] / count) as u8;
            target[target_offset + 1] = (sum[1] / count) as u8;
            target[target_offset + 2] = (sum[2] / count) as u8;
        }
    }
}

fn fill_circle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    circle: Circle,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 || circle.radius <= 0.0 {
        return;
    }

    let min_x = (circle.x - circle.radius - 1.0).floor().max(0.0) as u32;
    let max_x = (circle.x + circle.radius + 1.0)
        .ceil()
        .min(f64::from(width)) as u32;
    let min_y = (circle.y - circle.radius - 1.0).floor().max(0.0) as u32;
    let max_y = (circle.y + circle.radius + 1.0)
        .ceil()
        .min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let dx = f64::from(x) + 0.5 - circle.x;
            let dy = f64::from(y) + 0.5 - circle.y;
            let coverage = (circle.radius - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(
                    frame,
                    offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
}

fn fill_triangle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    triangle: Triangle,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 {
        return;
    }

    let min_x = triangle
        .a
        .x
        .min(triangle.b.x)
        .min(triangle.c.x)
        .floor()
        .max(0.0) as u32;
    let max_x = triangle
        .a
        .x
        .max(triangle.b.x)
        .max(triangle.c.x)
        .ceil()
        .min(f64::from(width)) as u32;
    let min_y = triangle
        .a
        .y
        .min(triangle.b.y)
        .min(triangle.c.y)
        .floor()
        .max(0.0) as u32;
    let max_y = triangle
        .a
        .y
        .max(triangle.b.y)
        .max(triangle.c.y)
        .ceil()
        .min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let mut covered = 0_u32;
            for sample_y in [0.25, 0.75] {
                for sample_x in [0.25, 0.75] {
                    if triangle_contains(
                        triangle,
                        Point {
                            x: f64::from(x) + sample_x,
                            y: f64::from(y) + sample_y,
                        },
                    ) {
                        covered += 1;
                    }
                }
            }
            if covered > 0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(frame, offset, color, (u32::from(alpha) * covered / 4) as u8);
            }
        }
    }
}

fn triangle_contains(triangle: Triangle, point: Point) -> bool {
    let d1 = edge_sign(point, triangle.a, triangle.b);
    let d2 = edge_sign(point, triangle.b, triangle.c);
    let d3 = edge_sign(point, triangle.c, triangle.a);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

fn edge_sign(point: Point, a: Point, b: Point) -> f64 {
    (point.x - b.x) * (a.y - b.y) - (a.x - b.x) * (point.y - b.y)
}

fn rounded_rect_coverage(x: f64, y: f64, rect: RoundedRect) -> f64 {
    let half_width = rect.width / 2.0;
    let half_height = rect.height / 2.0;
    let radius = rect.radius.min(half_width).min(half_height);
    let center_x = rect.x + half_width;
    let center_y = rect.y + half_height;
    let qx = (x - center_x).abs() - (half_width - radius);
    let qy = (y - center_y).abs() - (half_height - radius);
    let outside_x = qx.max(0.0);
    let outside_y = qy.max(0.0);
    let distance =
        (outside_x * outside_x + outside_y * outside_y).sqrt() + qx.max(qy).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn draw_overlay_text(
    font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    fallback_scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    if let Some(font) = font {
        font.draw_text(frame, width, height, x as i32, y as i32, text, color, alpha);
    } else {
        draw_bitmap_text(
            frame,
            width,
            height,
            x,
            y,
            fallback_scale,
            text,
            color,
            alpha,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bitmap_text(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    let scale = scale.max(1);
    let mut cursor = x;
    for ch in text.chars() {
        if let Some(glyph) = glyph(ch) {
            draw_glyph(frame, width, height, cursor, y, scale, glyph, color, alpha);
        }
        cursor = cursor.saturating_add(6 * scale);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    glyph: [u8; 7],
    color: [u8; 3],
    alpha: u8,
) {
    for (row, bits) in glyph.into_iter().enumerate() {
        for col in 0..5_u32 {
            if bits & (1_u8 << (4 - col)) == 0 {
                continue;
            }
            fill_solid_rect(
                frame,
                width,
                height,
                x + col * scale,
                y + row as u32 * scale,
                scale,
                scale,
                color,
                alpha,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_solid_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    cols: u32,
    rows: u32,
    color: [u8; 3],
    alpha: u8,
) {
    for py in y..y.saturating_add(rows).min(height) {
        for px in x..x.saturating_add(cols).min(width) {
            let offset = rgb_offset(width, px, py);
            blend_pixel(frame, offset, color, alpha);
        }
    }
}

fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        '0' => [
            0b11111, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b11111,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b11110, 0b00001, 0b00001, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b10010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01111, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b11110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        ' ' => [0; 7],
        _ => return None,
    })
}

fn bitmap_text_width(text: &str, scale: u32) -> u32 {
    let scale = scale.max(1);
    let chars = text.chars().count() as u32;
    if chars == 0 {
        0
    } else {
        chars * 6 * scale - scale
    }
}

fn blend_pixel(frame: &mut [u8], offset: usize, color: [u8; 3], alpha: u8) {
    let inverse = u16::from(255 - alpha);
    let alpha = u16::from(alpha);
    for channel in 0..3 {
        let source = u16::from(color[channel]) * alpha;
        let dest = u16::from(frame[offset + channel]) * inverse;
        frame[offset + channel] = ((source + dest + 127) / 255) as u8;
    }
}

fn rgb_offset(width: u32, x: u32, y: u32) -> usize {
    ((y * width + x) * 3) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_omits_hours_when_short() {
        assert_eq!(format_timestamp(Duration::from_secs(65)), "1:05");
    }

    #[test]
    fn timestamp_includes_hours_when_needed() {
        assert_eq!(format_timestamp(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn time_column_uses_hour_position_when_duration_has_hours() {
        let duration = Duration::from_secs(2 * 3600 + 6 * 60 + 44);

        assert_eq!(
            format_position_timestamp(Duration::ZERO, Some(duration)),
            "0:00:00"
        );
        assert_eq!(
            format_position_timestamp(Duration::from_secs(3600 + 37 * 60 + 38), Some(duration)),
            "1:37:38"
        );
        assert_eq!(time_column_template(Some(duration)), "0:00:00 / 2:06:44");
    }

    #[test]
    fn time_column_keeps_minute_position_for_short_duration() {
        let duration = Duration::from_secs(6 * 60 + 44);

        assert_eq!(
            format_position_timestamp(Duration::ZERO, Some(duration)),
            "0:00"
        );
        assert_eq!(time_column_template(Some(duration)), "0:00 / 6:44");
    }

    #[test]
    fn progress_pixels_clamps_to_width() {
        assert_eq!(
            progress_pixels(12, Duration::from_secs(30), Some(Duration::from_secs(10))),
            12
        );
    }

    #[test]
    fn paused_overlay_draws_play_button() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: true,
                visible: true,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: None,
                media_title: None,
            },
            &mut scratch,
            None,
        );

        let metrics = test_metrics(width, height);
        let offset = rgb_offset(
            width,
            metrics.inner_x + metrics.control_size / 2,
            metrics.control_y + metrics.control_size / 2,
        );
        assert!(frame[offset] > 180);
        assert!(frame[offset + 1] > 180);
        assert!(frame[offset + 2] > 180);
    }

    #[test]
    fn playing_overlay_draws_pause_button() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: false,
                visible: true,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: None,
                media_title: None,
            },
            &mut scratch,
            None,
        );

        let metrics = test_metrics(width, height);
        let offset = rgb_offset(
            width,
            metrics.inner_x + metrics.control_size / 3,
            metrics.control_y + metrics.control_size / 2,
        );
        assert!(frame[offset] > 180);
        assert!(frame[offset + 1] > 180);
        assert!(frame[offset + 2] > 180);
    }

    #[test]
    fn rendered_overlay_changes_bottom_pixels_only() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let before_top = frame[..(width * 20 * 3) as usize].to_vec();
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: true,
                visible: true,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: None,
                media_title: None,
            },
            &mut scratch,
            None,
        );

        assert_eq!(&frame[..before_top.len()], before_top.as_slice());
        assert!(
            frame
                .chunks_exact(3)
                .any(|pixel| pixel[0] > 200 && pixel[1] < 100 && pixel[2] < 100)
        );

        let metrics = test_metrics(width, height);
        let filled = progress_pixels(
            metrics.bar_width,
            Duration::from_secs(30),
            Some(Duration::from_secs(120)),
        );
        let handle_x = metrics.bar_x + filled;
        let handle_y = metrics.bar_y + metrics.bar_height / 2;
        let offset = rgb_offset(width, handle_x, handle_y);
        assert!(frame[offset] > 200);
        assert!(frame[offset + 1] < 120);
        assert!(frame[offset + 2] < 120);
    }

    #[test]
    fn acrylic_blur_softens_pixels_inside_rounded_rect_only() {
        let width = 80;
        let height = 40;
        let mut frame = vec![0_u8; (width * height * 3) as usize];
        for y in 0..height {
            for x in width / 2..width {
                let offset = rgb_offset(width, x, y);
                frame[offset] = 240;
                frame[offset + 1] = 240;
                frame[offset + 2] = 240;
            }
        }

        blur_rounded_rect(
            &mut frame,
            width,
            height,
            RoundedRect {
                x: 20.0,
                y: 20.0,
                width: 40.0,
                height: 12.0,
                radius: 4.0,
            },
            6,
        );

        let softened_offset = rgb_offset(width, 38, 26);
        assert!(frame[softened_offset] > 0);
        assert!(frame[softened_offset] < 240);

        let outside_offset = rgb_offset(width, 38, 5);
        assert_eq!(frame[outside_offset], 0);
        assert_eq!(frame[outside_offset + 1], 0);
        assert_eq!(frame[outside_offset + 2], 0);
    }

    #[test]
    fn text_width_counts_spacing_between_glyphs_only() {
        assert_eq!(bitmap_text_width("12", 2), 22);
    }

    #[test]
    fn hidden_overlay_leaves_frame_unchanged() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let before = frame.clone();
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: false,
                visible: false,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: None,
                media_title: None,
            },
            &mut scratch,
            None,
        );

        assert_eq!(frame, before);
    }

    #[test]
    fn status_message_can_render_without_playback_controls() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let before_top = frame[..(width * 40 * 3) as usize].to_vec();
        let before_bottom = frame[(width * 120 * 3) as usize..].to_vec();
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: false,
                visible: false,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: Some("MUTE ON"),
                media_title: None,
            },
            &mut scratch,
            None,
        );

        assert_ne!(&frame[..before_top.len()], before_top.as_slice());
        assert_eq!(
            &frame[(width * 120 * 3) as usize..],
            before_bottom.as_slice()
        );
    }

    #[test]
    fn media_title_renders_with_playback_controls() {
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];
        let before_top = frame[..(width * 40 * 3) as usize].to_vec();
        let mut scratch = String::new();

        render_overlay_rgb(
            &mut frame,
            width,
            height,
            100,
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: false,
                visible: true,
                audio_available: false,
                selected_audio: None,
                audio_picker_open: false,
                audio_labels: Vec::new(),
                subtitles_available: false,
                selected_subtitle: None,
                subtitle_picker_open: false,
                subtitle_labels: Vec::new(),
                status_message: None,
                media_title: Some("movie.mkv"),
            },
            &mut scratch,
            None,
        );

        assert_ne!(&frame[..before_top.len()], before_top.as_slice());
    }

    #[test]
    fn progress_hit_test_returns_ratio_on_bar() {
        let metrics = test_metrics(320, 180);
        let x = metrics.bar_x + metrics.bar_width / 2;
        let y = metrics.bar_y + metrics.bar_height / 2;

        let ratio =
            progress_hit_ratio_at_middle(metrics, hit_point(x, y)).expect("bar should be hittable");

        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn progress_hit_test_ignores_points_above_bar() {
        let metrics = test_metrics(320, 180);

        assert_eq!(
            progress_hit_ratio_at_middle(metrics, hit_point(metrics.bar_x, 0)),
            None
        );
    }

    #[test]
    fn progress_hit_test_ignores_side_padding() {
        let metrics = test_metrics(320, 180);
        let y = metrics.bar_y + metrics.bar_height / 2;

        assert_eq!(
            progress_hit_ratio_at_middle(metrics, hit_point(metrics.bar_x.saturating_sub(1), y)),
            None
        );
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point(metrics.bar_x + metrics.bar_width + 1, y)
            ),
            None
        );
    }

    #[test]
    fn progress_hit_test_keeps_visible_edge_handle_hittable() {
        let metrics = test_metrics(320, 180);
        let y = metrics.bar_y + metrics.bar_height / 2;
        let x = metrics.bar_x.saturating_sub(1);

        let ratio = progress_hit_ratio(
            metrics,
            hit_point(x, y),
            Duration::ZERO,
            Some(Duration::from_secs(120)),
        )
        .expect("visible start handle should be hittable");

        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn progress_hit_test_uses_clicked_cell_overlap() {
        let metrics = test_metrics_with_scale(1920, 1200, 120);
        let x = metrics.bar_x + metrics.bar_width / 2;
        let handle_radius = progress_handle_radius(metrics.bar_height).max(8) + 5;
        let handle_center_y = progress_handle_center_y(metrics);
        let cell_overlapping_from_below = hit_point_with_cell(
            x,
            HitboxRect {
                left: x,
                top: handle_center_y + handle_radius,
                right: x,
                bottom: handle_center_y + handle_radius,
            },
        );

        let ratio = progress_hit_ratio_at_middle(metrics, cell_overlapping_from_below)
            .expect("overlapping cell should be hittable");

        assert!((ratio - 0.5).abs() < 0.01);
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point_with_cell(
                    x,
                    HitboxRect {
                        left: x,
                        top: handle_center_y.saturating_sub(handle_radius + 20),
                        right: x,
                        bottom: handle_center_y.saturating_sub(handle_radius + 1),
                    },
                ),
            ),
            None
        );
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point_with_cell(
                    x,
                    HitboxRect {
                        left: x,
                        top: handle_center_y + handle_radius + 1,
                        right: x,
                        bottom: metrics.panel_y + metrics.panel_height,
                    },
                ),
            ),
            None
        );
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point_with_cell(
                    x,
                    HitboxRect {
                        left: x,
                        top: metrics.panel_y + metrics.panel_height,
                        right: x,
                        bottom: metrics.panel_y + metrics.panel_height,
                    },
                ),
            ),
            None
        );
    }

    #[test]
    fn playback_button_hit_test_uses_control_bounds() {
        let metrics = test_metrics(320, 180);

        assert!(playback_button_hit(
            metrics,
            hit_point(
                metrics.inner_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size / 2
            )
        ));
        assert!(!playback_button_hit(
            metrics,
            hit_point(metrics.time_x, metrics.control_y + metrics.control_size / 2)
        ));
    }

    #[test]
    fn subtitle_button_hit_test_toggles_picker() {
        let metrics = test_metrics_with_subtitles(320, 180);
        let rect = subtitle_button_rect(metrics);

        assert_eq!(
            subtitle_picker_action(
                metrics,
                hit_point(
                    rect.left + (rect.right - rect.left) / 2,
                    rect.top + (rect.bottom - rect.top) / 2,
                ),
                false,
                2,
            ),
            Some(SubtitlePickerAction::TogglePicker)
        );
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(rect.right + 1, rect.top), false, 2),
            None
        );
    }

    #[test]
    fn audio_button_hit_test_toggles_picker() {
        let metrics = test_metrics_with_audio_and_subtitles(320, 180);
        let rect = audio_button_rect(metrics);

        assert_eq!(
            audio_picker_action(
                metrics,
                hit_point(
                    rect.left + (rect.right - rect.left) / 2,
                    rect.top + (rect.bottom - rect.top) / 2,
                ),
                false,
                2,
            ),
            Some(AudioPickerAction::TogglePicker)
        );
        assert_eq!(
            audio_picker_action(metrics, hit_point(rect.right + 1, rect.top), false, 2),
            None
        );
    }

    #[test]
    fn subtitle_picker_width_expands_and_clamps_to_canvas() {
        let metrics = test_metrics_with_subtitles(320, 180);
        let anchor_x = track_picker_anchor_x(metrics);
        let short = track_picker_width(metrics, anchor_x, 20);
        let long = track_picker_width(metrics, anchor_x, 600);

        assert!(long > short);
        assert_eq!(long, track_picker_max_width(metrics, anchor_x));
    }

    #[test]
    fn subtitle_picker_selects_track_and_off_rows() {
        let metrics = test_metrics_with_subtitles(320, 180);
        let anchor_x = track_picker_anchor_x(metrics);
        let picker = track_picker_rect(
            metrics,
            anchor_x,
            2,
            track_picker_max_width(metrics, anchor_x),
            true,
        );
        let first = track_picker_track_rect(metrics, picker, 0);
        let second = track_picker_track_rect(metrics, picker, 1);
        let off = track_picker_off_rect(metrics, picker, 2);

        assert_eq!(
            subtitle_picker_action(metrics, hit_point(first.left + 1, first.top + 1), true, 2),
            Some(SubtitlePickerAction::SelectTrack(0))
        );
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(second.left + 1, second.top + 1), true, 2),
            Some(SubtitlePickerAction::SelectTrack(1))
        );
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(off.left + 1, off.top + 1), true, 2),
            Some(SubtitlePickerAction::SelectOff)
        );
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(metrics.bar_x, metrics.bar_y), true, 2),
            None
        );
    }

    #[test]
    fn audio_picker_selects_track_rows_without_off_row() {
        let metrics = test_metrics_with_audio_and_subtitles(320, 180);
        let anchor_x = track_picker_anchor_x(metrics);
        let picker = track_picker_rect(
            metrics,
            anchor_x,
            2,
            track_picker_max_width(metrics, anchor_x),
            false,
        );
        let first = track_picker_track_rect(metrics, picker, 0);
        let second = track_picker_track_rect(metrics, picker, 1);
        let off_space = track_picker_off_rect(metrics, picker, 2);

        assert_eq!(
            audio_picker_action(metrics, hit_point(first.left + 1, first.top + 1), true, 2),
            Some(AudioPickerAction::SelectTrack(0))
        );
        assert_eq!(
            audio_picker_action(metrics, hit_point(second.left + 1, second.top + 1), true, 2),
            Some(AudioPickerAction::SelectTrack(1))
        );
        assert_eq!(
            audio_picker_action(
                metrics,
                hit_point(off_space.left + 1, off_space.top + 1),
                true,
                2,
            ),
            None
        );
        assert_eq!(
            audio_picker_action(metrics, hit_point(metrics.bar_x, metrics.bar_y), true, 2),
            None
        );
    }

    #[test]
    fn playback_button_hit_test_uses_clicked_cell_overlap() {
        let metrics = test_metrics_with_scale(1920, 1200, 120);

        assert!(playback_button_hit(
            metrics,
            hit_point_with_cell(
                metrics.inner_x + metrics.control_size / 2,
                HitboxRect {
                    left: metrics.inner_x,
                    top: metrics.control_y + metrics.control_size - 1,
                    right: metrics.inner_x + metrics.control_size,
                    bottom: metrics.control_y + metrics.control_size + 20,
                },
            )
        ));
        assert!(!playback_button_hit(
            metrics,
            hit_point(metrics.time_x, metrics.control_y + metrics.control_size / 2)
        ));
    }

    #[test]
    fn overlay_uses_single_compact_row_across_sizes() {
        let small = test_metrics(320, 180);
        let large = test_metrics(1920, 1080);

        assert!(small.bar_x > small.time_x);
        assert!(large.bar_x > large.time_x);
        assert_eq!(small.panel_height, 34);
        assert_eq!(large.text_size, 18);
        assert!(large.panel_height <= 56);
    }

    #[test]
    fn progress_bar_end_gap_matches_start_gap_without_extra_controls() {
        let width = 320;
        let metrics = test_metrics(width, 180);
        let time_width = time_column_width(
            None,
            Some(Duration::from_secs(120)),
            metrics.fallback_text_scale,
        );
        let left_gap = metrics
            .bar_x
            .saturating_sub(metrics.time_x.saturating_add(time_width));
        let right_gap = width
            .saturating_sub(metrics.inner_x)
            .saturating_sub(metrics.bar_x.saturating_add(metrics.bar_width));

        assert_eq!(right_gap, left_gap);
    }

    #[test]
    fn progress_bar_keeps_matching_visual_gap_before_track_buttons() {
        let metrics = test_metrics_with_audio_and_subtitles(782, 586);
        let control_gap = control_gap_for_text(metrics.text_size);
        let time_width = time_column_width(
            None,
            Some(Duration::from_secs(120)),
            metrics.fallback_text_scale,
        );
        let left_gap = metrics
            .bar_x
            .saturating_sub(metrics.time_x.saturating_add(time_width));
        let right_gap = metrics
            .audio_x
            .saturating_sub(metrics.bar_x.saturating_add(metrics.bar_width));

        assert_eq!(left_gap, control_gap * 3);
        assert_eq!(right_gap, left_gap);
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point(metrics.bar_x.saturating_sub(1), metrics.bar_y),
            ),
            None
        );
        assert_eq!(
            progress_hit_ratio_at_middle(
                metrics,
                hit_point(metrics.bar_x + metrics.bar_width + 1, metrics.bar_y),
            ),
            None
        );
    }

    #[test]
    fn overlay_large_canvas_uses_normal_text_size() {
        let medium = test_metrics(640, 360);
        let large = test_metrics(1920, 1080);

        assert_eq!(medium.text_size, large.text_size);
        assert_eq!(large.text_size, 18);
    }

    #[test]
    fn overlay_high_density_scale_enlarges_controls() {
        let normal = test_metrics(1920, 1200);
        let high_density = test_metrics_with_scale(1920, 1200, 120);

        assert_eq!(normal.text_size, 18);
        assert_eq!(high_density.text_size, 22);
        assert!(high_density.panel_height > normal.panel_height);
        assert!(high_density.control_size > normal.control_size);
        assert!(high_density.bar_height > normal.bar_height);
    }

    #[test]
    fn top_message_gap_matches_bottom_overlay_gap() {
        let normal = test_metrics(1920, 1080);
        let high_density = test_metrics_with_scale(1920, 1200, 120);

        assert_eq!(
            top_message_y(1080, normal.text_size),
            bottom_panel_gap(1080, normal)
        );
        assert_eq!(
            top_message_y(1200, high_density.text_size),
            bottom_panel_gap(1200, high_density)
        );
    }

    fn test_metrics(width: u32, height: u32) -> OverlayMetrics {
        test_metrics_with_scale_and_controls(width, height, 100, false, false)
    }

    fn test_metrics_with_audio_and_subtitles(width: u32, height: u32) -> OverlayMetrics {
        test_metrics_with_scale_and_controls(width, height, 100, true, true)
    }

    fn test_metrics_with_subtitles(width: u32, height: u32) -> OverlayMetrics {
        test_metrics_with_scale_and_controls(width, height, 100, false, true)
    }

    fn hit_point(x: u32, y: u32) -> OverlayHitPoint {
        hit_point_with_cell(
            x,
            HitboxRect {
                left: x,
                top: y,
                right: x,
                bottom: y,
            },
        )
    }

    fn hit_point_with_cell(x: u32, cell: HitboxRect) -> OverlayHitPoint {
        OverlayHitPoint { x, cell }
    }

    fn progress_hit_ratio_at_middle(
        metrics: OverlayMetrics,
        point: OverlayHitPoint,
    ) -> Option<f64> {
        progress_hit_ratio(
            metrics,
            point,
            Duration::from_secs(60),
            Some(Duration::from_secs(120)),
        )
    }

    fn test_metrics_with_scale(width: u32, height: u32, scale_percent: u32) -> OverlayMetrics {
        test_metrics_with_scale_and_controls(width, height, scale_percent, false, false)
    }

    fn test_metrics_with_scale_and_controls(
        width: u32,
        height: u32,
        scale_percent: u32,
        audio_available: bool,
        subtitles_available: bool,
    ) -> OverlayMetrics {
        let text_size = text_size(width, height, scale_percent);
        let fallback_text_scale = fallback_text_scale(width, height, scale_percent);
        let text_height = 7 * fallback_text_scale;
        let time_width =
            time_column_width(None, Some(Duration::from_secs(120)), fallback_text_scale);
        OverlayMetrics::new(
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            time_width,
            audio_available,
            subtitles_available,
        )
    }

    fn bottom_panel_gap(height: u32, metrics: OverlayMetrics) -> u32 {
        height.saturating_sub(metrics.panel_y.saturating_add(metrics.panel_height))
    }
}
