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
            .saturating_sub(track_picker_layout(metrics, &labels[..visible], false, 0, None).top)
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
fn progress_bar_gap_matches_disabled_subtitle_control() {
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
    let right_gap = metrics
        .subtitle_x
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

fn progress_hit_ratio_at_middle(metrics: OverlayMetrics, point: OverlayHitPoint) -> Option<f64> {
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
    let time_width = time_column_width(None, Some(Duration::from_secs(120)), fallback_text_scale);
    OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        width as u16,
        terminal_rows,
        time_width,
        false,
        false,
        audio_available,
        subtitles_available,
    )
}

fn bottom_panel_gap(height: u32, metrics: OverlayMetrics) -> u32 {
    height.saturating_sub(metrics.panel_y.saturating_add(metrics.panel_height))
}
