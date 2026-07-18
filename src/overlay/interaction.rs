//! Pointer-to-action translation using the canonical overlay layout.

use std::time::Duration;

use super::{
    layout::{
        OverlayMetrics, audio_button_rect, midpoint_toward_lower_line, picker_text_y,
        progress_handle_radius, subtitle_button_rect, track_picker_track_rect,
    },
    state::{AudioPickerAction, HitboxRect, OverlayHitPoint, SubtitlePickerAction},
    timeline::progress_pixels,
};

pub(super) fn playback_button_hit(metrics: OverlayMetrics, point: OverlayHitPoint) -> bool {
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

pub(super) fn audio_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker: Option<HitboxRect>,
    audio_count: usize,
) -> Option<AudioPickerAction> {
    if let Some(picker) = picker
        && let Some(index) = track_picker_row_at_point(metrics, picker, point, audio_count)
    {
        return Some(AudioPickerAction::SelectTrack(index));
    }

    hitbox_intersects(point.cell, audio_button_rect(metrics))
        .then_some(AudioPickerAction::TogglePicker)
}

pub(super) fn subtitle_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker: Option<HitboxRect>,
    subtitle_count: usize,
) -> Option<SubtitlePickerAction> {
    if let Some(picker) = picker
        && let Some(index) =
            track_picker_row_at_point(metrics, picker, point, subtitle_count.saturating_add(1))
    {
        if index < subtitle_count {
            return Some(SubtitlePickerAction::SelectTrack(index));
        }
        return Some(SubtitlePickerAction::SelectOff);
    }

    hitbox_intersects(point.cell, subtitle_button_rect(metrics))
        .then_some(SubtitlePickerAction::TogglePicker)
}

fn track_picker_row_at_point(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    point: OverlayHitPoint,
    row_count: usize,
) -> Option<usize> {
    let first_row = track_picker_track_rect(metrics, picker, 0);
    if point.x < first_row.left || point.x >= first_row.right {
        return None;
    }

    (0..row_count).find(|index| {
        let hitbox = track_picker_row_hit_rect(metrics, picker, *index, row_count);
        point.y >= hitbox.top && point.y < hitbox.bottom
    })
}

fn track_picker_row_hit_rect(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    index: usize,
    row_count: usize,
) -> HitboxRect {
    let row = track_picker_track_rect(metrics, picker, index);
    let text_top = picker_text_y(metrics, row);
    let text_bottom = text_top.saturating_add(metrics.text_height);
    let top = if index == 0 {
        row.top
    } else {
        let previous = track_picker_track_rect(metrics, picker, index - 1);
        let previous_text_bottom =
            picker_text_y(metrics, previous).saturating_add(metrics.text_height);
        midpoint_toward_lower_line(previous_text_bottom, text_top)
    };
    let bottom = if index + 1 >= row_count {
        row.bottom
    } else {
        let next = track_picker_track_rect(metrics, picker, index + 1);
        midpoint_toward_lower_line(text_bottom, picker_text_y(metrics, next))
    };

    HitboxRect {
        left: row.left,
        top,
        right: row.right,
        bottom,
    }
}

pub(super) fn progress_hit_ratio(
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

pub(super) fn progress_ratio_for_x(metrics: OverlayMetrics, x: u32) -> f64 {
    let end_x = metrics.bar_x.saturating_add(metrics.bar_width);
    let x = x.clamp(metrics.bar_x, end_x);
    f64::from(x.saturating_sub(metrics.bar_x)) / f64::from(metrics.bar_width.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{layout::*, state::*, timeline::time_column_width};
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

    fn audio_picker_action(
        metrics: OverlayMetrics,
        point: OverlayHitPoint,
        picker_open: bool,
        track_count: usize,
    ) -> Option<AudioPickerAction> {
        let picker = picker_open.then(|| test_picker(metrics, track_count, false));
        super::audio_picker_action(metrics, point, picker, track_count)
    }

    fn subtitle_picker_action(
        metrics: OverlayMetrics,
        point: OverlayHitPoint,
        picker_open: bool,
        track_count: usize,
    ) -> Option<SubtitlePickerAction> {
        let picker = picker_open.then(|| test_picker(metrics, track_count, true));
        super::subtitle_picker_action(metrics, point, picker, track_count)
    }

    fn test_picker(metrics: OverlayMetrics, track_count: usize, include_off: bool) -> HitboxRect {
        let labels = (0..track_count)
            .map(|index| Arc::<str>::from(format!("Track {}", index + 1)))
            .collect::<Vec<_>>();
        track_picker_layout(metrics, &labels, include_off, None)
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
}
