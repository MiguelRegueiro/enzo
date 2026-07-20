//! Playback controls, progress indicator, and track-picker drawing.

use std::sync::Arc;

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    layout::{
        OverlayMetrics, picker_padding, picker_text_y, progress_handle_radius, rounded_radius,
        track_icon_dimensions, track_picker_layout, track_picker_track_rect,
        track_picker_visible_row_count,
    },
    raster::{
        Circle, Point, RoundedRect, Triangle, fill_circle, fill_rounded_rect, fill_triangle,
        stroke_rounded_rect,
    },
    state::HitboxRect,
    style::{ACCENT_COLOR, PANEL_COLOR, SHADOW_COLOR, TEXT_COLOR},
    text::{bitmap_text_width, draw_overlay_text},
};

pub(super) fn draw_playback_control(
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

pub(super) fn draw_audio_control(
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
    let (icon_width, icon_height) = track_icon_dimensions(metrics);
    let icon_x = metrics
        .audio_x
        .saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let icon_y = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);
    let radius = rounded_radius(icon_width, icon_height, icon_height / 4);
    let stroke = (icon_height / 8).max(2);
    stroke_rounded_rect(
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
        f64::from(stroke),
        TEXT_COLOR,
        alpha,
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

pub(super) fn draw_subtitle_control(
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
pub(super) fn draw_track_picker(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    labels: &[Arc<str>],
    selected_track: Option<usize>,
    scroll_offset: usize,
    focused_track: Option<usize>,
    include_off: bool,
    acrylic: &mut AcrylicScratch,
) {
    let row_count = labels.len().saturating_add(usize::from(include_off));
    let visible_count = track_picker_visible_row_count(metrics, row_count);
    let scroll_offset = scroll_offset.min(row_count.saturating_sub(visible_count));
    let picker = track_picker_layout(
        metrics,
        labels,
        include_off,
        scroll_offset,
        font.as_deref_mut(),
    );
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
        acrylic,
    );

    let pad = picker_padding(metrics);
    let marker_size = (metrics.text_size / 3).clamp(4, 7);
    let marker_x = picker.left.saturating_add(pad);
    let text_x = marker_x.saturating_add(marker_size).saturating_add(pad / 2);
    let text_width = picker.right.saturating_sub(text_x).saturating_sub(pad);
    for visible_index in 0..visible_count {
        let index = scroll_offset + visible_index;
        let row = track_picker_track_rect(metrics, picker, visible_index);
        let row_height = row.bottom.saturating_sub(row.top);
        let text_y = picker_text_y(metrics, row);
        let (label, alpha) = if let Some(label) = labels.get(index) {
            (label.as_ref(), 244)
        } else {
            ("Off", 210)
        };
        let selected = if index < labels.len() {
            selected_track == Some(index)
        } else {
            selected_track.is_none()
        };
        if focused_track == Some(index) {
            draw_picker_focus(
                frame,
                width,
                height,
                metrics,
                picker,
                visible_index,
                row_count > visible_count,
            );
        }
        if selected {
            draw_picker_marker(
                frame,
                width,
                height,
                marker_x,
                row.top,
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
            text_y,
            metrics.fallback_text_scale,
            &label,
            TEXT_COLOR,
            alpha,
        );
    }
    if row_count > visible_count {
        draw_picker_scrollbar(
            frame,
            width,
            height,
            picker,
            scroll_offset,
            visible_count,
            row_count,
        );
    }
}

fn draw_picker_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    picker: HitboxRect,
    scroll_offset: usize,
    visible_count: usize,
    row_count: usize,
) {
    let pad = 4;
    let bar_width = 3;
    let track_top = picker.top.saturating_add(pad);
    let track_bottom = picker.bottom.saturating_sub(pad);
    if track_bottom <= track_top || row_count == 0 {
        return;
    }
    let track_height = track_bottom - track_top;
    let thumb_height = ((track_height as usize * visible_count) / row_count)
        .max(8)
        .min(track_height as usize) as u32;
    let max_offset = row_count.saturating_sub(visible_count).max(1);
    let thumb_top = track_top
        + ((track_height - thumb_height) as usize * scroll_offset.min(max_offset) / max_offset)
            as u32;
    let x = picker.right.saturating_sub(pad).saturating_sub(bar_width);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(x),
            y: f64::from(thumb_top),
            width: f64::from(bar_width),
            height: f64::from(thumb_height),
            radius: f64::from(bar_width),
        },
        ACCENT_COLOR,
        232,
    );
}

fn draw_subtitle_icon(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    alpha: u8,
) {
    let (icon_width, icon_height) = track_icon_dimensions(metrics);
    let icon_x = metrics
        .subtitle_x
        .saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let icon_y = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);
    let radius = rounded_radius(icon_width, icon_height, icon_height / 4);
    let stroke = (icon_height / 8).max(2);
    stroke_rounded_rect(
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
        f64::from(stroke),
        TEXT_COLOR,
        alpha,
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

fn draw_picker_focus(
    frame: &mut [u8],
    width: u32,
    height: u32,
    metrics: OverlayMetrics,
    picker: HitboxRect,
    visible_index: usize,
    has_scrollbar: bool,
) {
    let pad = picker_padding(metrics);
    let left_pad = pad / 2;
    let right_pad = if has_scrollbar { pad.max(8) } else { left_pad };
    let row = track_picker_track_rect(metrics, picker, visible_index);
    let top = row.top.saturating_add(1);
    let row_height = row.bottom.saturating_sub(row.top).saturating_sub(2);
    let focus_width = row
        .right
        .saturating_sub(row.left)
        .saturating_sub(left_pad)
        .saturating_sub(right_pad);
    if row_height == 0 || focus_width == 0 {
        return;
    }
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(row.left.saturating_add(left_pad)),
            y: f64::from(top),
            width: f64::from(focus_width),
            height: f64::from(row_height),
            radius: f64::from(rounded_radius(
                row.right.saturating_sub(row.left),
                row_height,
                metrics.text_size / 4,
            )),
        },
        TEXT_COLOR,
        32,
    );
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

pub(super) fn draw_progress_handle(
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
