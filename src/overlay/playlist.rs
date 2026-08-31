//! Centered playlist menu layout, rendering, and pointer interaction.

use std::sync::Arc;

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    layout::{fallback_text_scale, rounded_radius, text_size},
    raster::{Circle, RoundedRect, fill_circle, fill_rounded_rect},
    state::{HitboxRect, OverlayHitPoint, PlaylistMenuAction, PlaylistMenuState},
    style::{OverlayPalette, PANEL_COLOR, TEXT_COLOR},
    text::{draw_overlay_text, fit_overlay_text, overlay_text_width},
};

const MAX_VISIBLE_ROWS: usize = 14;

#[derive(Clone, Copy)]
struct PlaylistGeometry {
    panel: HitboxRect,
    pad_x: u32,
    header_y: u32,
    rows_top: u32,
    row_pitch: u32,
    row_height: u32,
    text_height: u32,
    marker_size: u32,
    visible_count: usize,
    scrollbar_width: u32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_playlist_menu(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    state: &PlaylistMenuState,
    palette: OverlayPalette,
    acrylic: &mut AcrylicScratch,
) {
    let geometry = playlist_geometry(
        width,
        height,
        text_size,
        fallback_scale,
        text_height,
        &state.labels,
        font.as_deref_mut(),
    );
    let row_count = state.labels.len();
    let scroll_offset = state
        .scroll_offset
        .min(row_count.saturating_sub(geometry.visible_count));

    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(geometry.panel.left),
            y: f64::from(geometry.panel.top),
            width: f64::from(geometry.panel.right.saturating_sub(geometry.panel.left)),
            height: f64::from(geometry.panel.bottom.saturating_sub(geometry.panel.top)),
            radius: f64::from(rounded_radius(
                geometry.panel.right.saturating_sub(geometry.panel.left),
                geometry.panel.bottom.saturating_sub(geometry.panel.top),
                8,
            )),
        },
        PANEL_COLOR,
        224,
        acrylic,
    );

    let header_x = geometry.panel.left.saturating_add(geometry.pad_x);
    draw_overlay_text(
        font.as_deref_mut(),
        frame,
        width,
        height,
        header_x,
        geometry.header_y,
        fallback_scale,
        "Playlist",
        TEXT_COLOR,
        248,
    );

    let count = format!(
        "{} / {}",
        state.current.saturating_add(1).min(row_count),
        row_count
    );
    let count_width = overlay_text_width(&mut font, &count, fallback_scale);
    let count_x = geometry
        .panel
        .right
        .saturating_sub(geometry.pad_x)
        .saturating_sub(count_width);
    draw_overlay_text(
        font.as_deref_mut(),
        frame,
        width,
        height,
        count_x,
        geometry.header_y,
        fallback_scale,
        &count,
        TEXT_COLOR,
        196,
    );

    let index_digits = row_count.max(1).to_string().len();
    let has_scrollbar = row_count > geometry.visible_count;
    for visible_index in 0..geometry.visible_count {
        let index = scroll_offset + visible_index;
        let Some(label) = state.labels.get(index) else {
            break;
        };
        let row = playlist_row_rect(geometry, visible_index);
        if state.focus == Some(index) {
            draw_playlist_focus(frame, width, height, geometry, row, has_scrollbar);
        }

        let marker_x = row.left.saturating_add(geometry.pad_x);
        if state.current == index {
            fill_circle(
                frame,
                width,
                height,
                Circle {
                    x: f64::from(marker_x.saturating_add(geometry.marker_size / 2)),
                    y: f64::from(row.top.saturating_add(geometry.row_height / 2)),
                    radius: f64::from(geometry.marker_size / 2),
                },
                palette.accent,
                245,
            );
        }

        let text_x = marker_x
            .saturating_add(geometry.marker_size)
            .saturating_add(geometry.pad_x / 2);
        let right_pad = if has_scrollbar {
            geometry.pad_x.saturating_add(geometry.scrollbar_width)
        } else {
            geometry.pad_x
        };
        let text_width = row.right.saturating_sub(text_x).saturating_sub(right_pad);
        let numbered = format!("{:0index_digits$}  {}", index + 1, label);
        let label = fit_overlay_text(&mut font, &numbered, fallback_scale, text_width);
        let text_y = row
            .top
            .saturating_add(geometry.row_height.saturating_sub(geometry.text_height) / 2);
        draw_overlay_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_x,
            text_y,
            fallback_scale,
            &label,
            TEXT_COLOR,
            240,
        );
    }

    if has_scrollbar {
        draw_playlist_scrollbar(
            frame,
            width,
            height,
            geometry,
            scroll_offset,
            row_count,
            palette,
        );
    }
}

pub(super) fn playlist_menu_visible_row_count(
    width: u32,
    height: u32,
    scale_percent: u32,
    labels: &[Arc<str>],
    font: Option<&mut FontRenderer>,
) -> usize {
    measured_geometry(width, height, scale_percent, labels, font).visible_count
}

pub(super) fn playlist_menu_action(
    width: u32,
    height: u32,
    scale_percent: u32,
    point: OverlayHitPoint,
    scroll_offset: usize,
    labels: &[Arc<str>],
    font: Option<&mut FontRenderer>,
) -> Option<PlaylistMenuAction> {
    let geometry = measured_geometry(width, height, scale_percent, labels, font);
    if !point_in_rect(point, geometry.panel) {
        return Some(PlaylistMenuAction::Close);
    }
    playlist_row_at_point(geometry, point, scroll_offset, labels.len())
        .map(PlaylistMenuAction::Select)
}

pub(super) fn playlist_menu_hover_index(
    width: u32,
    height: u32,
    scale_percent: u32,
    point: OverlayHitPoint,
    scroll_offset: usize,
    labels: &[Arc<str>],
    font: Option<&mut FontRenderer>,
) -> Option<usize> {
    let geometry = measured_geometry(width, height, scale_percent, labels, font);
    playlist_row_at_point(geometry, point, scroll_offset, labels.len())
}

fn measured_geometry(
    width: u32,
    height: u32,
    scale_percent: u32,
    labels: &[Arc<str>],
    font: Option<&mut FontRenderer>,
) -> PlaylistGeometry {
    let text_size = text_size(width, height, scale_percent);
    let fallback_scale = fallback_text_scale(width, height, scale_percent);
    let font = font.and_then(|font| font.set_pixel_size(text_size).then_some(font));
    let text_height = font
        .as_ref()
        .map(|font| font.line_height())
        .unwrap_or(7 * fallback_scale);
    playlist_geometry(
        width,
        height,
        text_size,
        fallback_scale,
        text_height,
        labels,
        font,
    )
}

#[allow(clippy::too_many_arguments)]
fn playlist_geometry(
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    labels: &[Arc<str>],
    mut font: Option<&mut FontRenderer>,
) -> PlaylistGeometry {
    let compact = text_size <= 12 || height < 260;
    let pad_x = if compact { 8_u32 } else { 14 };
    let pad_y = if compact { 7_u32 } else { 12 };
    let header_gap = if compact { 5_u32 } else { 9 };
    let row_gap = if compact { 4_u32 } else { 8 };
    let row_pitch = text_height.saturating_add(row_gap).max(1);
    let row_height = row_pitch.saturating_sub(2).max(1);
    let marker_size = (text_size / 3).clamp(4, 7);
    let scrollbar_width = 3;
    let inset_x = (width / 28).clamp(2, 78).min(width.saturating_sub(1) / 2);
    let inset_y = (height / 12).clamp(4, 58).min(height.saturating_sub(1) / 2);
    let available_width = width.saturating_sub(inset_x.saturating_mul(2)).max(1);
    let available_height = height.saturating_sub(inset_y.saturating_mul(2)).max(1);

    let index_digits = labels.len().max(1).to_string().len();
    let index_sample = format!("{}  ", "9".repeat(index_digits));
    let index_width = overlay_text_width(&mut font, &index_sample, fallback_scale);
    let label_width = labels
        .iter()
        .map(|label| overlay_text_width(&mut font, label, fallback_scale))
        .max()
        .unwrap_or(0);
    let header_width = overlay_text_width(&mut font, "Playlist", fallback_scale)
        .saturating_add(pad_x)
        .saturating_add(overlay_text_width(
            &mut font,
            &format!("{} / {}", labels.len(), labels.len()),
            fallback_scale,
        ));
    let natural_width = pad_x
        .saturating_mul(2)
        .saturating_add(marker_size)
        .saturating_add(pad_x / 2)
        .saturating_add(index_width)
        .saturating_add(label_width)
        .saturating_add(scrollbar_width)
        .max(pad_x.saturating_mul(2).saturating_add(header_width));
    let width_cap = if width < 600 {
        available_width
    } else {
        available_width.min(text_size.saturating_mul(42).max(320))
    };
    let minimum_width = (width / 2).max(text_size.saturating_mul(18)).min(width_cap);
    let panel_width = natural_width.max(minimum_width).min(width_cap).max(1);

    let fixed_height = pad_y
        .saturating_mul(2)
        .saturating_add(text_height)
        .saturating_add(header_gap);
    let height_cap = available_height
        .min(height.saturating_mul(3) / 4)
        .max(fixed_height.saturating_add(row_pitch).min(available_height));
    let fitting_rows = height_cap.saturating_sub(fixed_height) / row_pitch;
    let visible_count = labels
        .len()
        .min(MAX_VISIBLE_ROWS)
        .min(fitting_rows.max(1) as usize);
    let panel_height = fixed_height
        .saturating_add(row_pitch.saturating_mul(visible_count as u32))
        .min(available_height)
        .max(1);
    let panel_left = width.saturating_sub(panel_width) / 2;
    let panel_top = height.saturating_sub(panel_height) / 2;
    let header_y = panel_top.saturating_add(pad_y);
    let rows_top = header_y
        .saturating_add(text_height)
        .saturating_add(header_gap);

    PlaylistGeometry {
        panel: HitboxRect {
            left: panel_left,
            top: panel_top,
            right: panel_left.saturating_add(panel_width),
            bottom: panel_top.saturating_add(panel_height),
        },
        pad_x,
        header_y,
        rows_top,
        row_pitch,
        row_height,
        text_height,
        marker_size,
        visible_count,
        scrollbar_width,
    }
}

fn playlist_row_rect(geometry: PlaylistGeometry, visible_index: usize) -> HitboxRect {
    let top = geometry
        .rows_top
        .saturating_add(geometry.row_pitch.saturating_mul(visible_index as u32));
    HitboxRect {
        left: geometry.panel.left,
        top,
        right: geometry.panel.right,
        bottom: top.saturating_add(geometry.row_height),
    }
}

fn playlist_row_at_point(
    geometry: PlaylistGeometry,
    point: OverlayHitPoint,
    scroll_offset: usize,
    row_count: usize,
) -> Option<usize> {
    let scroll_offset = scroll_offset.min(row_count.saturating_sub(geometry.visible_count));
    (0..geometry.visible_count).find_map(|visible_index| {
        let row = playlist_row_rect(geometry, visible_index);
        (point.y >= row.top
            && point.y < row.bottom.saturating_add(2)
            && point.x >= row.left
            && point.x < row.right)
            .then_some(scroll_offset + visible_index)
            .filter(|index| *index < row_count)
    })
}

fn point_in_rect(point: OverlayHitPoint, rect: HitboxRect) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn draw_playlist_focus(
    frame: &mut [u8],
    width: u32,
    height: u32,
    geometry: PlaylistGeometry,
    row: HitboxRect,
    has_scrollbar: bool,
) {
    let left_pad = geometry.pad_x / 2;
    let right_pad = if has_scrollbar {
        geometry.pad_x.saturating_add(geometry.scrollbar_width)
    } else {
        left_pad
    };
    let focus_width = row
        .right
        .saturating_sub(row.left)
        .saturating_sub(left_pad)
        .saturating_sub(right_pad);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(row.left.saturating_add(left_pad)),
            y: f64::from(row.top),
            width: f64::from(focus_width),
            height: f64::from(geometry.row_height),
            radius: f64::from(rounded_radius(focus_width, geometry.row_height, 5)),
        },
        TEXT_COLOR,
        32,
    );
}

fn draw_playlist_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    geometry: PlaylistGeometry,
    scroll_offset: usize,
    row_count: usize,
    palette: OverlayPalette,
) {
    let track_top = geometry.rows_top;
    let track_height = geometry
        .row_pitch
        .saturating_mul(geometry.visible_count as u32)
        .max(1);
    let thumb_height = ((u64::from(track_height) * geometry.visible_count as u64)
        / row_count.max(1) as u64)
        .max(8)
        .min(u64::from(track_height)) as u32;
    let max_offset = row_count.saturating_sub(geometry.visible_count).max(1);
    let thumb_range = track_height.saturating_sub(thumb_height);
    let thumb_top = track_top.saturating_add(
        (u64::from(thumb_range) * scroll_offset.min(max_offset) as u64 / max_offset as u64) as u32,
    );
    let x = geometry
        .panel
        .right
        .saturating_sub(geometry.pad_x / 2)
        .saturating_sub(geometry.scrollbar_width);
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(x),
            y: f64::from(thumb_top),
            width: f64::from(geometry.scrollbar_width),
            height: f64::from(thumb_height),
            radius: f64::from(geometry.scrollbar_width),
        },
        palette.accent,
        232,
    );
}

#[cfg(test)]
#[path = "tests/playlist.rs"]
mod tests;
