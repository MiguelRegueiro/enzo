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
const MIN_SCALE_PERCENT: u32 = 100;
const MAX_SCALE_PERCENT: u32 = 125;

#[derive(Clone, Copy)]
pub(crate) struct OverlayState {
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) paused: bool,
    pub(crate) visible: bool,
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
    pub(crate) duration: Option<Duration>,
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
        );
        progress_hit_ratio(metrics, point)
    }

    pub(crate) fn progress_ratio_from_x(
        &mut self,
        width: u32,
        height: u32,
        scale_percent: u32,
        duration: Option<Duration>,
        x: u32,
    ) -> f64 {
        let metrics = self.metrics(width, height, scale_percent, duration);
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
        );
        playback_button_hit(metrics, point)
    }

    fn metrics(
        &mut self,
        width: u32,
        height: u32,
        scale_percent: u32,
        duration: Option<Duration>,
    ) -> OverlayMetrics {
        overlay_metrics(width, height, scale_percent, duration, self.font.as_mut())
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
    time_x: u32,
    text_size: u32,
    fallback_text_scale: u32,
}

impl OverlayMetrics {
    fn new(
        width: u32,
        video_height: u32,
        text_size: u32,
        fallback_text_scale: u32,
        text_height: u32,
        time_width: u32,
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
        let bar_x = time_x
            .saturating_add(time_width)
            .saturating_add(control_gap)
            .min(content_right.saturating_sub(1));
        let bar_width = content_right.saturating_sub(bar_x).max(1);
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
            time_x,
            text_size,
            fallback_text_scale,
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
    );
    let panel_width = width
        .saturating_sub(metrics.inset_x.saturating_mul(2))
        .max(1);
    let panel_radius = rounded_radius(panel_width, metrics.panel_height, metrics.text_size);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(metrics.inset_x),
            y: f64::from(metrics.panel_y),
            width: f64::from(panel_width),
            height: f64::from(metrics.panel_height),
            radius: f64::from(panel_radius),
        },
        PANEL_COLOR,
        188,
    );

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
    scratch.push_str(&format_timestamp(state.position));
    scratch.push_str(" / ");
    if let Some(duration) = state.duration {
        scratch.push_str(&format_timestamp(duration));
    } else {
        scratch.push_str("--:--");
    }

    draw_playback_control(frame, width, height, metrics, state.paused);
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

    fill_rounded_rect(
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

fn progress_hit_ratio(metrics: OverlayMetrics, point: OverlayHitPoint) -> Option<f64> {
    let hit_radius = progress_handle_radius(metrics.bar_height).max(8) + 5;
    let bar_rect = HitboxRect {
        left: metrics.bar_x.saturating_sub(hit_radius),
        top: progress_handle_center_y(metrics).saturating_sub(hit_radius),
        right: metrics
            .bar_x
            .saturating_add(metrics.bar_width)
            .saturating_add(hit_radius),
        bottom: progress_handle_center_y(metrics).saturating_add(hit_radius),
    };
    if !hitbox_intersects(point.cell, bar_rect) {
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
        let text = format_timestamp(duration);
        format!("{text} / {text}")
    } else {
        "0:00 / --:--".to_string()
    }
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
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
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

        let ratio = progress_hit_ratio(metrics, hit_point(x, y)).expect("bar should be hittable");

        assert!((ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn progress_hit_test_ignores_points_above_bar() {
        let metrics = test_metrics(320, 180);

        assert_eq!(
            progress_hit_ratio(metrics, hit_point(metrics.bar_x, 0)),
            None
        );
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

        let ratio = progress_hit_ratio(metrics, cell_overlapping_from_below)
            .expect("overlapping cell should be hittable");

        assert!((ratio - 0.5).abs() < 0.01);
        assert_eq!(
            progress_hit_ratio(
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
            progress_hit_ratio(
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
            progress_hit_ratio(
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
        test_metrics_with_scale(width, height, 100)
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

    fn test_metrics_with_scale(width: u32, height: u32, scale_percent: u32) -> OverlayMetrics {
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
        )
    }

    fn bottom_panel_gap(height: u32, metrics: OverlayMetrics) -> u32 {
        height.saturating_sub(metrics.panel_y.saturating_add(metrics.panel_height))
    }
}
