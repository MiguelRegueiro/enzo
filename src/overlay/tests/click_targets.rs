use super::*;
use crate::overlay::{geometry::*, state::*, time_progress::time_column_width};
use std::{sync::Arc, time::Duration};

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
        progress_hit_ratio_at_middle(metrics, hit_point(metrics.bar_x + metrics.bar_width + 1, y)),
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

    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.playback_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size / 2
            )
        ),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(metrics.time_x, metrics.control_y + metrics.control_size / 2)
        ),
        None
    );
}

#[test]
fn playlist_control_hit_test_uses_transport_button_bounds() {
    let metrics = test_metrics_with_playlist(640, 360);
    let previous = previous_button_rect(metrics);
    let next = next_button_rect(metrics);

    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                previous.left + (previous.right - previous.left) / 2,
                previous.top + (previous.bottom - previous.top) / 2,
            )
        ),
        Some(TransportControlAction::Previous)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                next.left + (next.right - next.left) / 2,
                next.top + (next.bottom - next.top) / 2,
            )
        ),
        Some(TransportControlAction::Next)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.playback_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size / 2
            )
        ),
        Some(TransportControlAction::Playback)
    );
}

#[test]
fn subtitle_button_hit_test_toggles_picker() {
    let metrics = test_metrics_with_subtitles(640, 360);
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
        subtitle_picker_action(
            metrics,
            hit_point(metrics.subtitle_x, metrics.control_y),
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
    let metrics = test_metrics_with_audio_and_subtitles(640, 360);
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
        audio_picker_action(
            metrics,
            hit_point(metrics.audio_x, metrics.control_y),
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
fn subtitle_picker_selects_track_and_off_rows() {
    let metrics = test_metrics_with_subtitles(320, 180);
    let picker = test_picker(metrics, 2, true);
    let first = track_picker_track_rect(metrics, picker, 0);
    let second = track_picker_track_rect(metrics, picker, 1);
    let off = track_picker_track_rect(metrics, picker, 2);

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
        subtitle_picker_action(metrics, hit_point(second.left + 1, second.top), true, 2),
        Some(SubtitlePickerAction::SelectTrack(1))
    );
    assert_eq!(
        subtitle_picker_action(metrics, hit_point(off.left + 1, off.top), true, 2),
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
    let picker = test_picker(metrics, 2, false);
    let first = track_picker_track_rect(metrics, picker, 0);
    let second = track_picker_track_rect(metrics, picker, 1);
    let off_space = track_picker_track_rect(metrics, picker, 2);

    assert_eq!(
        audio_picker_action(metrics, hit_point(first.left + 1, first.top + 1), true, 2),
        Some(AudioPickerAction::SelectTrack(0))
    );
    assert_eq!(
        audio_picker_action(metrics, hit_point(second.left + 1, second.top + 1), true, 2),
        Some(AudioPickerAction::SelectTrack(1))
    );
    assert_eq!(
        audio_picker_action(metrics, hit_point(second.left + 1, second.top), true, 2),
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
fn scrolled_audio_picker_maps_visible_rows_to_scrolled_tracks() {
    let metrics = test_metrics_with_audio_and_subtitles(320, 180);
    let labels = (0..20)
        .map(|index| Arc::<str>::from(format!("Track {}", index + 1)))
        .collect::<Vec<_>>();
    let offset = 5;
    let picker = track_picker_layout(metrics, &labels, false, offset, None);
    let first_visible = track_picker_track_rect(metrics, picker, 0);

    assert_eq!(
        super::audio_picker_action(
            metrics,
            hit_point(first_visible.left + 1, first_visible.top + 1),
            Some(picker),
            labels.len(),
            offset,
            track_picker_visible_row_count(metrics, labels.len()),
        ),
        Some(AudioPickerAction::SelectTrack(offset))
    );
}

#[test]
fn terminal_aligned_picker_entry_centers_select_expected_rows() {
    let metrics =
        test_metrics_with_scale_controls_and_terminal_rows(1920, 1080, 24, 100, false, true);
    let picker = test_picker(metrics, 4, true);

    for (index, expected) in [
        SubtitlePickerAction::SelectTrack(0),
        SubtitlePickerAction::SelectTrack(1),
        SubtitlePickerAction::SelectTrack(2),
        SubtitlePickerAction::SelectTrack(3),
        SubtitlePickerAction::SelectOff,
    ]
    .into_iter()
    .enumerate()
    {
        let row = track_picker_track_rect(metrics, picker, index);
        let visible_entry_center = row
            .top
            .saturating_add(row.bottom.saturating_sub(row.top) / 2);

        assert_eq!(
            subtitle_picker_action(
                metrics,
                hit_point(row.left + 1, visible_entry_center),
                true,
                4,
            ),
            Some(expected)
        );
    }
}

#[test]
fn picker_gap_hitboxes_split_between_visible_lines() {
    let metrics =
        test_metrics_with_scale_controls_and_terminal_rows(1920, 1080, 24, 100, false, true);
    let picker = test_picker(metrics, 2, true);
    let first = track_picker_track_rect(metrics, picker, 0);
    let second = track_picker_track_rect(metrics, picker, 1);
    let first_text_top = picker_text_y(metrics, first);
    let first_text_bottom = first_text_top.saturating_add(metrics.text_height);
    let second_text_top = picker_text_y(metrics, second);
    let gap_midpoint = midpoint_toward_lower_line(first_text_bottom, second_text_top);
    let x = first.left + 1;

    assert_eq!(
        track_picker_row_hit_rect(metrics, picker, 0, 3).bottom,
        gap_midpoint
    );
    assert_eq!(
        track_picker_row_hit_rect(metrics, picker, 1, 3).top,
        gap_midpoint
    );
    for y in [
        first_text_top,
        first_text_bottom.saturating_sub(1),
        gap_midpoint.saturating_sub(1),
    ] {
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(x, y), true, 2),
            Some(SubtitlePickerAction::SelectTrack(0))
        );
    }
    for y in [gap_midpoint, second_text_top] {
        assert_eq!(
            subtitle_picker_action(metrics, hit_point(x, y), true, 2),
            Some(SubtitlePickerAction::SelectTrack(1))
        );
    }
}

#[test]
fn picker_actions_ignore_space_outside_the_visible_picker() {
    let metrics = test_metrics_with_subtitles(320, 180);
    let picker = test_picker(metrics, 2, true);
    let first = track_picker_track_rect(metrics, picker, 0);
    let y = first.top + first.bottom.saturating_sub(first.top) / 2;

    assert!(picker.left > metrics.inset_x);
    assert_eq!(
        subtitle_picker_action(metrics, hit_point(picker.left - 1, y), true, 2),
        None
    );
    assert_eq!(
        subtitle_picker_action(metrics, hit_point(picker.left, y), true, 2),
        Some(SubtitlePickerAction::SelectTrack(0))
    );
}

#[test]
fn picker_hover_reports_same_rows_as_click_selection() {
    let metrics = test_metrics_with_subtitles(320, 180);
    let picker = test_picker(metrics, 3, true);
    let second = track_picker_track_rect(metrics, picker, 1);
    let point = hit_point(
        picker.left,
        second.top + second.bottom.saturating_sub(second.top) / 2,
    );

    assert_eq!(
        track_picker_hover_index(metrics, point, Some(picker), 4, 0, 4),
        Some(1)
    );
    assert_eq!(
        subtitle_picker_action(metrics, point, true, 3),
        Some(SubtitlePickerAction::SelectTrack(1))
    );
}

#[test]
fn playback_button_hit_test_accepts_a_bounded_edge_slop() {
    let metrics = test_metrics_with_scale(1920, 1200, 120);

    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.playback_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size,
            )
        ),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.playback_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size + 8,
            )
        ),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(metrics.time_x, metrics.control_y + metrics.control_size / 2)
        ),
        None
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.playback_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size + 9
            )
        ),
        None
    );
}

#[test]
fn playlist_buttons_partition_inner_gaps_and_keep_bounded_outer_edges() {
    let metrics = test_metrics_with_playlist(1920, 1200);

    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.previous_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size + 8,
            )
        ),
        Some(TransportControlAction::Previous)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.next_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size + 8,
            )
        ),
        Some(TransportControlAction::Next)
    );
    let gap_x = metrics.playback_x
        + metrics.control_size
        + (metrics.next_x - metrics.playback_x - metrics.control_size) / 2;
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(gap_x, metrics.control_y + metrics.control_size / 2)
        ),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(
                metrics.next_x + metrics.control_size / 2,
                metrics.control_y + metrics.control_size + 9,
            )
        ),
        None
    );
}

#[test]
fn transport_action_resolves_cluster_at_nearest_control_boundary() {
    let metrics = test_metrics_with_playlist(1920, 1200);
    let y = metrics.control_y + metrics.control_size / 2;
    let gap_x = metrics.playback_x
        + metrics.control_size
        + (metrics.next_x - metrics.playback_x - metrics.control_size) / 2;

    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(metrics.playback_x + metrics.control_size / 2, y)
        ),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point(metrics.next_x + metrics.control_size / 2, y)
        ),
        Some(TransportControlAction::Next)
    );
    assert_eq!(
        transport_control_action(metrics, hit_point(gap_x, y)),
        Some(TransportControlAction::Playback)
    );
    assert_eq!(
        transport_control_action(metrics, hit_point(gap_x + 1, y)),
        Some(TransportControlAction::Next)
    );
    assert_eq!(
        transport_control_action(
            metrics,
            hit_point_with_cell(
                gap_x,
                HitboxRect {
                    left: metrics.playback_x + metrics.control_size + 1,
                    top: y,
                    right: metrics.next_x.saturating_sub(1),
                    bottom: y,
                },
            )
        ),
        Some(TransportControlAction::Playback)
    );
}

#[test]
fn cell_aligned_transport_maps_near_pause_and_next_icon_clicks_correctly() {
    let width = 1920;
    let height = 1200;

    for terminal_cols in [80, 120, 240] {
        let metrics = test_metrics_with_playlist_columns(width, height, terminal_cols);
        let y = metrics.control_y + metrics.control_size / 2;
        let bar_width = (metrics.control_size / 5).max(2);
        let pause_gap = (metrics.control_size / 7).max(2);
        let pause_width = bar_width.saturating_mul(2).saturating_add(pause_gap);
        let pause_right = metrics
            .playback_x
            .saturating_add(metrics.control_size.saturating_sub(pause_width) / 2)
            .saturating_add(pause_width);
        let next_left = metrics
            .next_x
            .saturating_add(metrics.control_size.saturating_mul(29) / 100);
        let near_pause_x = pause_right.saturating_add(next_left.saturating_sub(pause_right) / 3);

        assert_eq!(
            transport_control_action(
                metrics,
                terminal_cell_point_for_x(width, terminal_cols, near_pause_x, y),
            ),
            Some(TransportControlAction::Playback),
            "terminal width {terminal_cols}",
        );
        assert_eq!(
            transport_control_action(
                metrics,
                terminal_cell_point_for_x(width, terminal_cols, next_left + 1, y),
            ),
            Some(TransportControlAction::Next),
            "terminal width {terminal_cols}",
        );
    }
}

fn audio_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker_open: bool,
    track_count: usize,
) -> Option<AudioPickerAction> {
    let picker = picker_open.then(|| test_picker(metrics, track_count, false));
    super::audio_picker_action(metrics, point, picker, track_count, 0, track_count)
}

fn subtitle_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker_open: bool,
    track_count: usize,
) -> Option<SubtitlePickerAction> {
    let picker = picker_open.then(|| test_picker(metrics, track_count, true));
    super::subtitle_picker_action(metrics, point, picker, track_count, 0, track_count + 1)
}

fn test_picker(metrics: OverlayMetrics, track_count: usize, include_off: bool) -> HitboxRect {
    let labels = (0..track_count)
        .map(|index| Arc::<str>::from(format!("Track {}", index + 1)))
        .collect::<Vec<_>>();
    track_picker_layout(metrics, &labels, include_off, 0, None)
}

fn test_metrics(width: u32, height: u32) -> OverlayMetrics {
    test_metrics_with_scale_and_controls(width, height, 100, false, false)
}

fn test_metrics_with_audio_and_subtitles(width: u32, height: u32) -> OverlayMetrics {
    test_metrics_with_scale_and_controls(width, height, 100, true, true)
}

fn test_metrics_with_playlist(width: u32, height: u32) -> OverlayMetrics {
    test_metrics_with_playlist_columns(width, height, width as u16)
}

fn test_metrics_with_playlist_columns(
    width: u32,
    height: u32,
    terminal_cols: u16,
) -> OverlayMetrics {
    let text_size = text_size(width, height, 100);
    let fallback_text_scale = fallback_text_scale(width, height, 100);
    let text_height = 7 * fallback_text_scale;
    let time_width = time_column_width(None, Some(Duration::from_secs(120)), fallback_text_scale);
    OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        terminal_cols,
        height as u16,
        time_width,
        true,
        true,
        false,
        false,
    )
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

fn terminal_cell_point_for_x(
    width: u32,
    terminal_cols: u16,
    clicked_x: u32,
    y: u32,
) -> OverlayHitPoint {
    let columns = u32::from(terminal_cols.max(1));
    let column = (u64::from(clicked_x.min(width.saturating_sub(1)))
        .saturating_mul(u64::from(columns))
        / u64::from(width.max(1)))
    .min(u64::from(columns.saturating_sub(1))) as u32;
    let left = (u64::from(column).saturating_mul(u64::from(width)) / u64::from(columns)) as u32;
    let right = (u64::from(column.saturating_add(1))
        .saturating_mul(u64::from(width))
        .div_ceil(u64::from(columns))
        .saturating_sub(1)
        .min(u64::from(width.saturating_sub(1)))) as u32;
    let center = (u64::from(column.saturating_mul(2).saturating_add(1))
        .saturating_mul(u64::from(width))
        / u64::from(columns.saturating_mul(2))) as u32;
    hit_point_with_cell(
        center,
        HitboxRect {
            left,
            top: y,
            right,
            bottom: y,
        },
    )
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
