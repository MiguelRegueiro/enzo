//! Canonical overlay geometry shared by drawing and pointer interaction.

use std::{sync::Arc, time::Duration};

use crate::font::FontRenderer;

use super::{state::HitboxRect, text::bitmap_text_width, time_progress::time_column_width};

const MIN_SCALE_PERCENT: u32 = 100;
const MAX_SCALE_PERCENT: u32 = 125;

#[derive(Clone, Copy)]
pub(super) struct OverlayMetrics {
    pub(super) panel_y: u32,
    pub(super) panel_height: u32,
    pub(super) inset_x: u32,
    pub(super) text_y: u32,
    pub(super) bar_x: u32,
    pub(super) bar_y: u32,
    pub(super) bar_width: u32,
    pub(super) bar_height: u32,
    pub(super) control_size: u32,
    pub(super) control_y: u32,
    pub(super) previous_x: u32,
    pub(super) playback_x: u32,
    pub(super) next_x: u32,
    pub(super) audio_x: u32,
    pub(super) subtitle_x: u32,
    pub(super) time_x: u32,
    pub(super) text_size: u32,
    pub(super) text_height: u32,
    pub(super) fallback_text_scale: u32,
    pub(super) canvas_height: u32,
    pub(super) terminal_rows: u16,
    pub(super) picker_terminal_row_span: u16,
    pub(super) panel_right: u32,
}

impl OverlayMetrics {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        width: u32,
        video_height: u32,
        text_size: u32,
        fallback_text_scale: u32,
        text_height: u32,
        terminal_cols: u16,
        terminal_rows: u16,
        time_width: u32,
        playlist_previous_available: bool,
        playlist_next_available: bool,
        audio_available: bool,
        _subtitles_available: bool,
    ) -> Self {
        let bar_height = bar_height_for_text(text_size).min(video_height.max(1));
        let vertical_pad = vertical_padding_for_text(text_size);
        let outer_y = outer_padding_for_text(text_size);
        let control_size = control_size_for_text(text_size, text_height);
        let control_gap = control_gap_for_text(text_size);
        let handle_radius = progress_handle_radius(bar_height);
        let picker_line_pitch = text_size
            .max(text_height)
            .max(7 * fallback_text_scale)
            .saturating_add(6);
        let terminal_rows = terminal_rows.max(1);
        let picker_terminal_row_span = (u64::from(picker_line_pitch)
            .saturating_mul(u64::from(terminal_rows))
            .div_ceil(u64::from(video_height.max(1))))
        .clamp(1, u64::from(u16::MAX)) as u16;
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
        let playlist_available = playlist_previous_available || playlist_next_available;
        let (previous_x, playback_x, next_x) = if playlist_available {
            cell_aligned_transport_positions(
                width,
                terminal_cols,
                inner_x,
                control_size,
                control_gap,
            )
        } else {
            let playback_x = cell_aligned_control_x(width, terminal_cols, inner_x, control_size);
            (playback_x, playback_x, playback_x)
        };
        let transport_right = if playlist_available {
            next_x.saturating_add(control_size)
        } else {
            playback_x.saturating_add(control_size)
        };
        let time_x = transport_right
            .saturating_add(control_gap)
            .min(width.saturating_sub(1));
        let content_right = width.saturating_sub(inner_x).max(inner_x.saturating_add(1));
        // Keep the subtitle control visible even when no tracks are available.
        let controls = u32::from(audio_available).saturating_add(1);
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
        let subtitle_x = next_control_x;
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
            text_y,
            bar_x,
            bar_y,
            bar_width,
            bar_height,
            control_size,
            control_y,
            previous_x,
            playback_x,
            next_x,
            audio_x,
            subtitle_x,
            time_x,
            text_size,
            text_height,
            fallback_text_scale,
            canvas_height: video_height,
            terminal_rows,
            picker_terminal_row_span,
            panel_right,
        }
    }
}

fn cell_aligned_transport_positions(
    width: u32,
    terminal_cols: u16,
    inner_x: u32,
    control_size: u32,
    control_gap: u32,
) -> (u32, u32, u32) {
    let columns = u32::from(terminal_cols.max(1));
    if columns < 3 {
        let playback_x = inner_x
            .saturating_add(control_size)
            .saturating_add(control_gap);
        let next_x = playback_x
            .saturating_add(control_size)
            .saturating_add(control_gap);
        return (inner_x, playback_x, next_x);
    }

    let desired_center = inner_x.saturating_add(control_size / 2);
    let mut first_column = terminal_column_for_x(width, columns, desired_center);
    let desired_step = control_size.saturating_add(control_gap);
    let mut column_step = ((u64::from(desired_step)
        .saturating_mul(u64::from(columns))
        .saturating_add(u64::from(width.max(1)) / 2)
        / u64::from(width.max(1))) as u32)
        .max(1);
    column_step = column_step.min((columns.saturating_sub(1) / 2).max(1));
    first_column = first_column.min(columns.saturating_sub(column_step.saturating_mul(2) + 1));

    let previous_x = control_x_for_terminal_column(width, columns, first_column, control_size);
    let playback_x = control_x_for_terminal_column(
        width,
        columns,
        first_column.saturating_add(column_step),
        control_size,
    );
    let next_x = control_x_for_terminal_column(
        width,
        columns,
        first_column.saturating_add(column_step.saturating_mul(2)),
        control_size,
    );
    (previous_x, playback_x, next_x)
}

fn cell_aligned_control_x(
    width: u32,
    terminal_cols: u16,
    desired_x: u32,
    control_size: u32,
) -> u32 {
    let columns = u32::from(terminal_cols.max(1));
    let desired_center = desired_x.saturating_add(control_size / 2);
    let column = terminal_column_for_x(width, columns, desired_center);
    control_x_for_terminal_column(width, columns, column, control_size)
}

fn terminal_column_for_x(width: u32, columns: u32, x: u32) -> u32 {
    (u64::from(x.min(width.saturating_sub(1))) * u64::from(columns) / u64::from(width.max(1)))
        .min(u64::from(columns.saturating_sub(1))) as u32
}

fn control_x_for_terminal_column(width: u32, columns: u32, column: u32, control_size: u32) -> u32 {
    let center = (u64::from(column.saturating_mul(2).saturating_add(1)) * u64::from(width)
        / u64::from(columns.saturating_mul(2).max(1))) as u32;
    center
        .saturating_sub(control_size / 2)
        .min(width.saturating_sub(control_size))
}

#[allow(clippy::too_many_arguments)]
fn top_message_y(height: u32, text_size: u32) -> u32 {
    outer_padding_for_text(text_size).min(height.saturating_sub(1))
}

pub(super) fn top_message_stack_y(
    height: u32,
    text_size: u32,
    text_height: u32,
    stack_index: u32,
) -> u32 {
    let pad_y = (vertical_padding_for_text(text_size) / 2).max(4);
    top_message_y(height, text_size)
        .saturating_add(
            text_height
                .saturating_add(pad_y.saturating_mul(3))
                .saturating_mul(stack_index),
        )
        .min(height.saturating_sub(1))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn overlay_metrics(
    width: u32,
    height: u32,
    terminal_cols: u16,
    terminal_rows: u16,
    scale_percent: u32,
    duration: Option<Duration>,
    playlist_previous_available: bool,
    playlist_next_available: bool,
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
        terminal_cols,
        terminal_rows,
        time_width,
        playlist_previous_available,
        playlist_next_available,
        audio_available,
        subtitles_available,
    )
}

pub(super) fn audio_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    icon_button_rect(metrics.audio_x, metrics)
}

pub(super) fn subtitle_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    icon_button_rect(metrics.subtitle_x, metrics)
}

pub(super) fn previous_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    transport_button_rect(metrics.previous_x, metrics)
}

pub(super) fn playback_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    transport_button_rect(metrics.playback_x, metrics)
}

pub(super) fn next_button_rect(metrics: OverlayMetrics) -> HitboxRect {
    transport_button_rect(metrics.next_x, metrics)
}

fn transport_button_rect(x: u32, metrics: OverlayMetrics) -> HitboxRect {
    HitboxRect {
        left: x,
        top: metrics.control_y,
        right: x.saturating_add(metrics.control_size),
        bottom: metrics.control_y.saturating_add(metrics.control_size),
    }
}

pub(super) fn track_icon_dimensions(metrics: OverlayMetrics) -> (u32, u32) {
    (
        (metrics.control_size * 9 / 10).max(14),
        (metrics.control_size * 3 / 4).max(12),
    )
}

fn icon_button_rect(x: u32, metrics: OverlayMetrics) -> HitboxRect {
    let (icon_width, icon_height) = track_icon_dimensions(metrics);
    let icon_left = x.saturating_add(metrics.control_size.saturating_sub(icon_width) / 2);
    let icon_top = metrics
        .control_y
        .saturating_add(metrics.control_size.saturating_sub(icon_height) / 2);

    HitboxRect {
        left: x.min(icon_left),
        top: metrics.control_y.min(icon_top),
        right: x
            .saturating_add(metrics.control_size)
            .max(icon_left.saturating_add(icon_width)),
        bottom: metrics
            .control_y
            .saturating_add(metrics.control_size)
            .max(icon_top.saturating_add(icon_height)),
    }
}

fn track_picker_anchor_x(metrics: OverlayMetrics) -> u32 {
    metrics.panel_right.saturating_sub(metrics.control_size)
}

pub(super) fn track_picker_layout(
    metrics: OverlayMetrics,
    labels: &[Arc<str>],
    include_off: bool,
    scroll_offset: usize,
    mut font: Option<&mut FontRenderer>,
) -> HitboxRect {
    let max_label_width = labels
        .iter()
        .map(AsRef::as_ref)
        .chain(include_off.then_some("Off"))
        .map(|label| {
            font.as_deref_mut()
                .map(|font| font.text_width(label))
                .unwrap_or_else(|| bitmap_text_width(label, metrics.fallback_text_scale))
        })
        .max()
        .unwrap_or(0);
    let anchor_x = track_picker_anchor_x(metrics);
    let picker_width = track_picker_width(metrics, anchor_x, max_label_width);
    let row_count = labels.len().saturating_add(usize::from(include_off));
    let visible_count = track_picker_visible_row_count(metrics, row_count);
    let offset = scroll_offset.min(row_count.saturating_sub(visible_count));
    track_picker_rect(
        metrics,
        anchor_x,
        row_count.saturating_sub(offset).min(visible_count),
        picker_width,
    )
}

pub(super) fn track_picker_visible_row_count(metrics: OverlayMetrics, row_count: usize) -> usize {
    let pad = picker_padding(metrics);
    let desired_rows_bottom = metrics
        .panel_y
        .saturating_sub(track_picker_gap_for_text(metrics.text_size))
        .saturating_sub(pad);
    let end_terminal_row = terminal_row_at_or_before_y(metrics, desired_rows_bottom);
    let start_terminal_row = terminal_row_at_or_after_y(
        metrics,
        track_picker_top_margin(metrics).saturating_add(pad),
    );
    let row_span = u32::from(metrics.picker_terminal_row_span).max(1);
    let rows = end_terminal_row.saturating_sub(start_terminal_row) / row_span;
    row_count.min(rows.max(1) as usize)
}

fn track_picker_rect(
    metrics: OverlayMetrics,
    anchor_x: u32,
    track_count: usize,
    picker_width: u32,
) -> HitboxRect {
    let pad = picker_padding(metrics);
    let row_count = track_count;
    let right = anchor_x
        .saturating_add(metrics.control_size)
        .max(picker_width);
    let left = right.saturating_sub(picker_width);
    let desired_rows_bottom = metrics
        .panel_y
        .saturating_sub(track_picker_gap_for_text(metrics.text_size))
        .saturating_sub(pad);
    let end_terminal_row = terminal_row_at_or_before_y(metrics, desired_rows_bottom);
    let terminal_row_count =
        (row_count as u32).saturating_mul(u32::from(metrics.picker_terminal_row_span));
    let start_terminal_row = end_terminal_row.saturating_sub(terminal_row_count);
    let rows_top = terminal_row_boundary(metrics, start_terminal_row);
    let rows_bottom = terminal_row_boundary(metrics, end_terminal_row);
    let top = rows_top
        .saturating_sub(pad)
        .max(track_picker_top_margin(metrics));
    let bottom = rows_bottom.saturating_add(pad);
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
        .max(scaled_normal_pixels(144, metrics.text_size))
        .max(metrics.control_size);
    desired.min(track_picker_max_width(metrics, anchor_x).max(1))
}

fn track_picker_max_width(metrics: OverlayMetrics, anchor_x: u32) -> u32 {
    anchor_x
        .saturating_add(metrics.control_size)
        .saturating_sub(metrics.inset_x)
        .max(metrics.control_size)
}

pub(super) fn track_picker_track_rect(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    index: usize,
) -> HitboxRect {
    let pad = picker_padding(metrics);
    let rows_top = if picker.top == 0 {
        0
    } else {
        picker.top.saturating_add(pad)
    };
    let start_terminal_row = terminal_row_at_or_before_y(metrics, rows_top);
    let row_offset = (index as u32).saturating_mul(u32::from(metrics.picker_terminal_row_span));
    let top = terminal_row_boundary(metrics, start_terminal_row.saturating_add(row_offset));
    let bottom = terminal_row_boundary(
        metrics,
        start_terminal_row
            .saturating_add(row_offset)
            .saturating_add(u32::from(metrics.picker_terminal_row_span)),
    );
    HitboxRect {
        left: picker.left,
        top,
        right: picker.right,
        bottom,
    }
}

pub(super) fn picker_padding(metrics: OverlayMetrics) -> u32 {
    (horizontal_padding_for_text(metrics.text_size) / 2).max(6)
}

fn track_picker_top_margin(metrics: OverlayMetrics) -> u32 {
    terminal_row_boundary(metrics, 2)
        .max(vertical_padding_for_text(metrics.text_size).saturating_mul(2))
}

pub(super) fn picker_text_y(metrics: OverlayMetrics, row: HitboxRect) -> u32 {
    row.top.saturating_add(
        row.bottom
            .saturating_sub(row.top)
            .saturating_sub(metrics.text_height)
            / 2,
    )
}

pub(super) fn midpoint_toward_lower_line(upper: u32, lower: u32) -> u32 {
    upper.saturating_add(lower.saturating_sub(upper).div_ceil(2))
}

fn terminal_row_boundary(metrics: OverlayMetrics, terminal_row: u32) -> u32 {
    (u64::from(terminal_row.min(u32::from(metrics.terminal_rows)))
        .saturating_mul(u64::from(metrics.canvas_height))
        / u64::from(metrics.terminal_rows))
    .min(u64::from(metrics.canvas_height)) as u32
}

fn terminal_row_at_or_before_y(metrics: OverlayMetrics, y: u32) -> u32 {
    let rows = u64::from(metrics.terminal_rows);
    let height = u64::from(metrics.canvas_height.max(1));
    let y = u64::from(y.min(metrics.canvas_height));
    y.saturating_add(1)
        .saturating_mul(rows)
        .saturating_sub(1)
        .checked_div(height)
        .unwrap_or(0)
        .min(rows) as u32
}

fn terminal_row_at_or_after_y(metrics: OverlayMetrics, y: u32) -> u32 {
    let rows = u64::from(metrics.terminal_rows);
    let height = u64::from(metrics.canvas_height.max(1));
    let y = u64::from(y.min(metrics.canvas_height));
    y.saturating_mul(rows)
        .saturating_add(height.saturating_sub(1))
        .checked_div(height)
        .unwrap_or(0)
        .min(rows) as u32
}

pub(super) fn text_size(width: u32, video_height: u32, scale_percent: u32) -> u32 {
    let base = if width >= 420 && video_height >= 240 {
        18
    } else {
        12
    };
    scaled_overlay_pixels(base, scale_percent)
}

pub(super) fn fallback_text_scale(width: u32, video_height: u32, scale_percent: u32) -> u32 {
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

pub(super) fn vertical_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(11, text_size),
        _ => 8,
    }
}

pub(super) fn horizontal_padding_for_text(text_size: u32) -> u32 {
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

fn outer_padding_for_text(text_size: u32) -> u32 {
    match text_size {
        18.. => scaled_normal_pixels(6, text_size),
        _ => 4,
    }
}

pub(super) fn progress_handle_radius(bar_height: u32) -> u32 {
    (bar_height * 7 / 5).clamp(6, 14)
}

pub(super) fn rounded_radius(width: u32, height: u32, wanted: u32) -> u32 {
    wanted.max(1).min(width.max(1) / 2).min(height.max(1) / 2)
}

#[cfg(test)]
#[path = "tests/geometry.rs"]
mod tests;
