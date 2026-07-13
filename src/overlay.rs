use std::time::Duration;

use crate::font::FontRenderer;

const PANEL_COLOR: [u8; 3] = [18, 18, 22];
const TRACK_COLOR: [u8; 3] = [82, 82, 91];
const ACCENT_COLOR: [u8; 3] = [239, 68, 68];
const TEXT_COLOR: [u8; 3] = [250, 250, 250];
const SHADOW_COLOR: [u8; 3] = [0, 0, 0];

#[derive(Clone, Copy)]
pub(crate) struct OverlayState {
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) paused: bool,
    pub(crate) visible: bool,
}

pub(crate) struct PlaybackOverlay {
    scratch: String,
    font: Option<FontRenderer>,
}

impl PlaybackOverlay {
    pub(crate) fn new() -> Self {
        Self {
            scratch: String::new(),
            font: FontRenderer::open_default(18),
        }
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        state: OverlayState,
    ) {
        render_overlay_rgb(
            frame,
            width,
            height,
            state,
            &mut self.scratch,
            self.font.as_mut(),
        );
    }
}

#[derive(Clone, Copy)]
struct OverlayMetrics {
    panel_y: u32,
    panel_height: u32,
    inset_x: u32,
    inner_x: u32,
    text_y: u32,
    bar_y: u32,
    bar_width: u32,
    bar_height: u32,
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
    ) -> Self {
        let bar_height = bar_height_for_text(text_size).min(video_height.max(1));
        let text_gap = text_gap_for_text(text_size);
        let vertical_pad = vertical_padding_for_text(text_size);
        let outer_y = outer_padding_for_text(text_size);
        let handle_radius = progress_handle_radius(bar_height);
        let bottom_pad = vertical_pad.max(
            handle_radius
                .saturating_sub(bar_height / 2)
                .saturating_add(3),
        );
        let panel_height = vertical_pad
            .saturating_add(text_height)
            .saturating_add(text_gap)
            .saturating_add(bar_height)
            .saturating_add(bottom_pad)
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
        let bar_width = width.saturating_sub(inner_x.saturating_mul(2)).max(1);
        let text_y = panel_y.saturating_add(vertical_pad);
        let bar_y = text_y.saturating_add(text_height).saturating_add(text_gap);

        Self {
            panel_y,
            panel_height,
            inset_x,
            inner_x,
            text_y,
            bar_y,
            bar_width,
            bar_height,
            text_size,
            fallback_text_scale,
        }
    }
}

fn render_overlay_rgb(
    frame: &mut [u8],
    width: u32,
    height: u32,
    state: OverlayState,
    scratch: &mut String,
    font: Option<&mut FontRenderer>,
) {
    if width == 0 || height == 0 || frame.len() < (width as usize * height as usize * 3) {
        return;
    }
    if !state.visible {
        return;
    }

    let text_size = text_size(width, height);
    let fallback_text_scale = fallback_text_scale(width, height);
    let mut font = if let Some(font) = font {
        if font.set_pixel_size(text_size) {
            Some(font)
        } else {
            None
        }
    } else {
        None
    };
    let text_height = font
        .as_ref()
        .map(|font| font.line_height())
        .unwrap_or(7 * fallback_text_scale);
    let metrics = OverlayMetrics::new(width, height, text_size, fallback_text_scale, text_height);
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
            x: f64::from(metrics.inner_x),
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
                x: f64::from(metrics.inner_x),
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

    draw_overlay_text(
        font.as_mut().map(|font| &mut **font),
        frame,
        width,
        height,
        metrics.inner_x,
        metrics.text_y,
        metrics.fallback_text_scale,
        scratch,
        TEXT_COLOR,
        238,
    );

    if state.paused {
        let label = "PAUSED";
        let label_width = overlay_text_width(font.as_mut().map(|font| &mut **font), label, metrics);
        if metrics.bar_width > label_width + metrics.fallback_text_scale * 4 {
            let label_x = metrics.inner_x + metrics.bar_width - label_width;
            draw_overlay_text(
                font,
                frame,
                width,
                height,
                label_x,
                metrics.text_y,
                metrics.fallback_text_scale,
                label,
                TEXT_COLOR,
                220,
            );
        }
    }
}

fn draw_progress_handle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    filled: u32,
) {
    let radius = progress_handle_radius(metrics.bar_height);
    let center_x = f64::from(metrics.inner_x + filled.min(metrics.bar_width));
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

fn text_size(width: u32, video_height: u32) -> u32 {
    if width >= 960 && video_height >= 540 {
        24
    } else if width >= 420 && video_height >= 240 {
        18
    } else {
        12
    }
}

fn fallback_text_scale(width: u32, video_height: u32) -> u32 {
    if width >= 960 && video_height >= 540 {
        3
    } else if width >= 420 && video_height >= 240 {
        2
    } else {
        1
    }
}

fn bar_height_for_text(text_size: u32) -> u32 {
    match text_size {
        24.. => 8,
        18.. => 6,
        _ => 5,
    }
}

fn text_gap_for_text(text_size: u32) -> u32 {
    match text_size {
        24.. => 10,
        18.. => 8,
        _ => 6,
    }
}

fn vertical_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        24.. => 14,
        18.. => 11,
        _ => 8,
    }
}

fn horizontal_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        24.. => 24,
        18.. => 18,
        _ => 12,
    }
}

fn outer_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        24.. => 8,
        18.. => 6,
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

fn overlay_text_width(font: Option<&mut FontRenderer>, text: &str, metrics: OverlayMetrics) -> u32 {
    font.map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, metrics.fallback_text_scale))
}

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
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
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
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: true,
                visible: true,
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
        let handle_x = metrics.inner_x + filled;
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
            OverlayState {
                position: Duration::from_secs(30),
                duration: Some(Duration::from_secs(120)),
                paused: false,
                visible: false,
            },
            &mut scratch,
            None,
        );

        assert_eq!(frame, before);
    }

    #[test]
    fn overlay_top_padding_stays_consistent_across_sizes() {
        let small = test_metrics(320, 180);
        let large = test_metrics(1920, 1080);

        assert_eq!(small.text_y - small.panel_y, 8);
        assert_eq!(large.text_y - large.panel_y, 14);
        assert!(large.panel_height <= 76);
    }

    fn test_metrics(width: u32, height: u32) -> OverlayMetrics {
        let text_size = text_size(width, height);
        let fallback_text_scale = fallback_text_scale(width, height);
        let text_height = 7 * fallback_text_scale;
        OverlayMetrics::new(width, height, text_size, fallback_text_scale, text_height)
    }
}
