//! Pointer-to-action translation using the canonical overlay layout.

use std::time::Duration;

use super::{
    layout::{
        OverlayMetrics, audio_button_rect, midpoint_toward_lower_line, next_button_rect,
        picker_text_y, playback_button_rect, previous_button_rect, progress_handle_radius,
        subtitle_button_rect, track_picker_track_rect,
    },
    state::{
        AudioPickerAction, HitboxRect, OverlayHitPoint, SubtitlePickerAction,
        TransportControlAction,
    },
    timeline::progress_pixels,
};

pub(super) fn transport_control_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
) -> Option<TransportControlAction> {
    let playlist_controls_visible = metrics.previous_x != metrics.playback_x;
    let playback = playback_button_rect(metrics);
    if !playlist_controls_visible {
        return transport_row_hit(metrics, point, playback)
            .then_some(TransportControlAction::Playback);
    }

    let previous = previous_button_rect(metrics);
    let next = next_button_rect(metrics);
    let cluster = HitboxRect {
        left: previous.left,
        top: playback.top,
        right: next.right,
        bottom: playback.bottom,
    };
    if !transport_row_hit(metrics, point, cluster) {
        return None;
    }

    let previous_center = rect_center_x(previous);
    let playback_center = rect_center_x(playback);
    let next_center = rect_center_x(next);
    let previous_boundary =
        previous_center.saturating_add(playback_center.saturating_sub(previous_center) / 2);
    let next_boundary =
        playback_center.saturating_add(next_center.saturating_sub(playback_center) / 2);
    if point.x < previous_boundary {
        Some(TransportControlAction::Previous)
    } else if point.x <= next_boundary {
        Some(TransportControlAction::Playback)
    } else {
        Some(TransportControlAction::Next)
    }
}

pub(super) fn audio_picker_action(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker: Option<HitboxRect>,
    audio_count: usize,
    scroll_offset: usize,
    visible_count: usize,
) -> Option<AudioPickerAction> {
    if let Some(picker) = picker
        && let Some(index) = track_picker_row_at_point(
            metrics,
            picker,
            point,
            audio_count,
            scroll_offset,
            visible_count,
        )
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
    scroll_offset: usize,
    visible_count: usize,
) -> Option<SubtitlePickerAction> {
    if let Some(picker) = picker
        && let Some(index) = track_picker_row_at_point(
            metrics,
            picker,
            point,
            subtitle_count.saturating_add(1),
            scroll_offset,
            visible_count,
        )
    {
        if index < subtitle_count {
            return Some(SubtitlePickerAction::SelectTrack(index));
        }
        return Some(SubtitlePickerAction::SelectOff);
    }

    hitbox_intersects(point.cell, subtitle_button_rect(metrics))
        .then_some(SubtitlePickerAction::TogglePicker)
}

pub(super) fn track_picker_hover_index(
    metrics: OverlayMetrics,
    point: OverlayHitPoint,
    picker: Option<HitboxRect>,
    row_count: usize,
    scroll_offset: usize,
    visible_count: usize,
) -> Option<usize> {
    track_picker_row_at_point(
        metrics,
        picker?,
        point,
        row_count,
        scroll_offset,
        visible_count,
    )
}

fn track_picker_row_at_point(
    metrics: OverlayMetrics,
    picker: HitboxRect,
    point: OverlayHitPoint,
    row_count: usize,
    scroll_offset: usize,
    visible_count: usize,
) -> Option<usize> {
    let first_row = track_picker_track_rect(metrics, picker, 0);
    if point.x < first_row.left || point.x >= first_row.right {
        return None;
    }

    let last_visible = visible_count.min(row_count.saturating_sub(scroll_offset));
    (0..last_visible).find_map(|visible_index| {
        let hitbox = track_picker_row_hit_rect(metrics, picker, visible_index, last_visible);
        (point.y >= hitbox.top && point.y < hitbox.bottom).then_some(scroll_offset + visible_index)
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

fn transport_row_hit(metrics: OverlayMetrics, point: OverlayHitPoint, rect: HitboxRect) -> bool {
    let vertical_slop = 8.min(metrics.control_size / 2).max(3);
    let hitbox = HitboxRect {
        left: rect.left,
        top: rect.top.saturating_sub(vertical_slop),
        right: rect.right,
        bottom: rect.bottom.saturating_add(vertical_slop),
    };
    point.x >= hitbox.left && point.x <= hitbox.right && hitbox_intersects(point.cell, hitbox)
}

fn rect_center_x(rect: HitboxRect) -> u32 {
    rect.left
        .saturating_add(rect.right.saturating_sub(rect.left) / 2)
}

pub(super) fn progress_ratio_for_x(metrics: OverlayMetrics, x: u32) -> f64 {
    let end_x = metrics.bar_x.saturating_add(metrics.bar_width);
    let x = x.clamp(metrics.bar_x, end_x);
    f64::from(x.saturating_sub(metrics.bar_x)) / f64::from(metrics.bar_width.max(1))
}

#[cfg(test)]
#[path = "tests/interaction.rs"]
mod tests;
