//! Canonical overlay geometry shared by drawing and pointer interaction.

use std::{sync::Arc, time::Duration};

use crate::font::FontRenderer;

use super::{state::HitboxRect, text::bitmap_text_width, timeline::time_column_width};

const MIN_SCALE_PERCENT: u32 = 100;
const MAX_SCALE_PERCENT: u32 = 125;

#[derive(Clone, Copy)]
pub(super) struct OverlayMetrics {
    pub(super) panel_y: u32,
    pub(super) panel_height: u32,
    pub(super) inset_x: u32,
    pub(super) inner_x: u32,
    pub(super) text_y: u32,
    pub(super) bar_x: u32,
    pub(super) bar_y: u32,
    pub(super) bar_width: u32,
    pub(super) bar_height: u32,
    pub(super) control_size: u32,
    pub(super) control_y: u32,
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
        terminal_rows: u16,
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
            text_height,
            fallback_text_scale,
            canvas_height: video_height,
            terminal_rows,
            picker_terminal_row_span,
            panel_right,
        }
    }
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
    terminal_rows: u16,
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
        terminal_rows,
        time_width,
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
mod tests {
    use super::*;
    use crate::overlay::{interaction::progress_hit_ratio, state::*};
    use std::time::Duration;

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
    fn track_picker_height_clamps_to_available_rows() {
        let metrics = test_metrics_with_subtitles(320, 180);
        let labels = (0..40)
            .map(|index| Arc::<str>::from(format!("Track {}", index + 1)))
            .collect::<Vec<_>>();
        let visible = track_picker_visible_row_count(metrics, labels.len() + 1);
        let picker = track_picker_layout(metrics, &labels, true, 0, None);

        assert!(visible < labels.len() + 1);
        assert_eq!(
            picker.bottom.saturating_sub(picker.top),
            track_picker_layout(metrics, &labels[..visible], false, 0, None)
                .bottom
                .saturating_sub(
                    track_picker_layout(metrics, &labels[..visible], false, 0, None).top
                )
        );
        assert!(picker.top < metrics.panel_y);
    }

    #[test]
    fn track_picker_scroll_offset_keeps_panel_height_stable() {
        let metrics = test_metrics_with_subtitles(320, 180);
        let labels = (0..40)
            .map(|index| Arc::<str>::from(format!("Track {}", index + 1)))
            .collect::<Vec<_>>();
        let top = track_picker_layout(metrics, &labels, true, 0, None);
        let scrolled = track_picker_layout(metrics, &labels, true, 10, None);

        assert_eq!(top.top, scrolled.top);
        assert_eq!(top.bottom, scrolled.bottom);
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

    #[test]
    fn top_message_stack_keeps_rows_below_each_other() {
        let height = 360;
        let text_size = 18;
        let text_height = 14;
        let pad_y = (vertical_padding_for_text(text_size) / 2).max(4);
        let title_bottom = top_message_y(height, text_size)
            .saturating_add(text_height)
            .saturating_add(pad_y.saturating_mul(2));
        let row_pitch = text_height + pad_y * 3;

        assert_eq!(
            top_message_stack_y(height, text_size, text_height, 1),
            title_bottom + pad_y
        );
        assert_eq!(
            top_message_stack_y(height, text_size, text_height, 2),
            top_message_y(height, text_size) + row_pitch * 2
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
        let y = cell
            .top
            .saturating_add(cell.bottom.saturating_sub(cell.top) / 2);
        OverlayHitPoint { x, y, cell }
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
        test_metrics_with_scale_controls_and_terminal_rows(
            width,
            height,
            height as u16,
            scale_percent,
            audio_available,
            subtitles_available,
        )
    }

    fn test_metrics_with_scale_controls_and_terminal_rows(
        width: u32,
        height: u32,
        terminal_rows: u16,
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
            terminal_rows,
            time_width,
            audio_available,
            subtitles_available,
        )
    }

    fn bottom_panel_gap(height: u32, metrics: OverlayMetrics) -> u32 {
        height.saturating_sub(metrics.panel_y.saturating_add(metrics.panel_height))
    }
}
