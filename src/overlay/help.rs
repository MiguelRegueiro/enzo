//! Playback help overlay layout and rendering.

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    layout::{fallback_text_scale, rounded_radius, text_size},
    raster::{RoundedRect, fill_rounded_rect},
    style::{ACCENT_COLOR, PANEL_COLOR, TEXT_COLOR, TRACK_COLOR},
    text::{bitmap_text_width, draw_overlay_text, fit_overlay_text, overlay_text_width},
};

#[derive(Clone, Copy)]
struct HelpSection {
    title: &'static str,
    rows: &'static [HelpRow],
    wide_column: HelpColumn,
}

#[derive(Clone, Copy)]
struct HelpRow {
    key: &'static str,
    action: &'static str,
}

#[derive(Clone, Copy)]
enum HelpLine {
    Title(&'static str),
    Section(&'static str),
    Row(HelpRow),
}

#[derive(Clone, Copy)]
enum HelpColumn {
    Left,
    Right,
    Info,
}

#[derive(Clone, Copy)]
struct HelpGeometry {
    panel: RoundedRect,
    pad_x: u32,
    content_y: u32,
    content_height: u32,
    column_count: usize,
    column_widths: [u32; MAX_HELP_COLUMNS],
    column_gap: u32,
    line_pitch: u32,
    key_pad_x: u32,
    key_pad_y: u32,
    key_height: u32,
    scrollbar_width: u32,
}

const MAX_HELP_COLUMNS: usize = 3;

const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Playback",
        wide_column: HelpColumn::Left,
        rows: &[
            HelpRow {
                key: "Space",
                action: "Play/pause",
            },
            HelpRow {
                key: "Right click",
                action: "Play/pause",
            },
            HelpRow {
                key: "PgUp/Dn",
                action: "Previous/next",
            },
            HelpRow {
                key: "p",
                action: "Playlist menu",
            },
            HelpRow {
                key: "q",
                action: "Quit",
            },
            HelpRow {
                key: "Q",
                action: "Quit without saving",
            },
        ],
    },
    HelpSection {
        title: "Seek",
        wide_column: HelpColumn::Left,
        rows: &[
            HelpRow {
                key: "Left/Right",
                action: "Seek -/+5s",
            },
            HelpRow {
                key: "Down/Up",
                action: "Seek -/+60s",
            },
        ],
    },
    HelpSection {
        title: "Audio",
        wide_column: HelpColumn::Right,
        rows: &[
            HelpRow {
                key: "9/0",
                action: "Volume -/+2%",
            },
            HelpRow {
                key: "Wheel",
                action: "Volume/menu",
            },
            HelpRow {
                key: "m",
                action: "Mute",
            },
            HelpRow {
                key: "a",
                action: "Audio menu",
            },
        ],
    },
    HelpSection {
        title: "Subtitles",
        wide_column: HelpColumn::Right,
        rows: &[
            HelpRow {
                key: "v",
                action: "Show/hide",
            },
            HelpRow {
                key: "s",
                action: "Subtitle menu",
            },
            HelpRow {
                key: "Drop sub",
                action: "Load sub",
            },
        ],
    },
    HelpSection {
        title: "Info",
        wide_column: HelpColumn::Info,
        rows: &[
            HelpRow {
                key: "i",
                action: "Info",
            },
            HelpRow {
                key: "I",
                action: "Pin info",
            },
            HelpRow {
                key: "?",
                action: "Toggle help",
            },
            HelpRow {
                key: "Esc",
                action: "Close",
            },
        ],
    },
];

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_help_panel(
    font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    scroll_offset: usize,
    acrylic: &mut AcrylicScratch,
) {
    let mut font = font;
    let geometry = help_geometry(width, height, text_size, text_height, fallback_scale);
    let columns = help_columns(geometry.column_count);
    let max_offset = scroll_limit_for_columns(&columns, geometry);
    let scroll_offset = scroll_offset.min(max_offset);

    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        geometry.panel,
        PANEL_COLOR,
        224,
        acrylic,
    );
    let mut column_x = geometry.panel.x as u32 + geometry.pad_x;
    for (column_index, lines) in columns.iter().enumerate() {
        let column_width = geometry.column_widths[column_index];
        draw_help_column(
            font.as_deref_mut(),
            frame,
            width,
            height,
            fallback_scale,
            geometry,
            column_x,
            column_width,
            lines,
            scroll_offset,
        );
        column_x = column_x
            .saturating_add(column_width)
            .saturating_add(geometry.column_gap);
    }

    if max_offset > 0 {
        draw_scrollbar(frame, width, height, geometry, scroll_offset, max_offset);
    }
}

pub(super) fn help_scroll_limit(
    width: u32,
    height: u32,
    scale_percent: u32,
    mut font: Option<&mut FontRenderer>,
) -> usize {
    let text_size = text_size(width, height, scale_percent);
    let fallback_scale = fallback_text_scale(width, height, scale_percent);
    let text_height = font
        .as_mut()
        .and_then(|font| font.set_pixel_size(text_size).then(|| font.line_height()))
        .unwrap_or(7 * fallback_scale);
    let geometry = help_geometry(width, height, text_size, text_height, fallback_scale);
    let columns = help_columns(geometry.column_count);
    scroll_limit_for_columns(&columns, geometry)
}

#[allow(clippy::too_many_arguments)]
fn draw_help_column(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    fallback_scale: u32,
    geometry: HelpGeometry,
    column_x: u32,
    column_width: u32,
    lines: &[HelpLine],
    scroll_offset: usize,
) {
    let visible_lines = visible_line_count(geometry);
    let column_right = column_x.saturating_add(column_width);
    let action_gap = (geometry.key_pad_x / 2).max(6);

    for (visible_index, line) in lines
        .iter()
        .skip(scroll_offset)
        .take(visible_lines)
        .enumerate()
    {
        let y = geometry
            .content_y
            .saturating_add(geometry.line_pitch.saturating_mul(visible_index as u32));
        if y.saturating_add(geometry.key_height) > geometry.content_y + geometry.content_height {
            break;
        }
        match *line {
            HelpLine::Title(title) => {
                draw_overlay_text(
                    font.as_deref_mut(),
                    frame,
                    width,
                    height,
                    column_x,
                    y.saturating_add(geometry.key_pad_y),
                    fallback_scale,
                    title,
                    TEXT_COLOR,
                    248,
                );
            }
            HelpLine::Section(title) => {
                draw_overlay_text(
                    font.as_deref_mut(),
                    frame,
                    width,
                    height,
                    column_x,
                    y.saturating_add(geometry.key_pad_y),
                    fallback_scale,
                    title,
                    ACCENT_COLOR,
                    238,
                );
            }
            HelpLine::Row(row) => {
                let key_width = help_row_key_width(&mut font, row, fallback_scale, geometry)
                    .min(column_width.saturating_sub(1));
                let action_x = column_x
                    .saturating_add(key_width)
                    .saturating_add(action_gap);
                let action_width = column_right.saturating_sub(action_x).max(1);
                draw_help_row(
                    font.as_deref_mut(),
                    frame,
                    width,
                    height,
                    fallback_scale,
                    geometry,
                    column_x,
                    key_width,
                    action_x,
                    action_width,
                    y,
                    row,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_help_row(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    fallback_scale: u32,
    geometry: HelpGeometry,
    x: u32,
    key_width: u32,
    action_x: u32,
    action_width: u32,
    y: u32,
    row: HelpRow,
) {
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(x),
            y: f64::from(y),
            width: f64::from(key_width),
            height: f64::from(geometry.key_height),
            radius: f64::from(rounded_radius(key_width, geometry.key_height, 5)),
        },
        TRACK_COLOR,
        114,
    );

    let key = fit_overlay_text(
        &mut font,
        row.key,
        fallback_scale,
        key_width.saturating_sub(4),
    );
    let key_text_width = overlay_text_width(&mut font, &key, fallback_scale);
    draw_overlay_text(
        font.as_deref_mut(),
        frame,
        width,
        height,
        x.saturating_add(key_width.saturating_sub(key_text_width) / 2),
        y.saturating_add(geometry.key_pad_y),
        fallback_scale,
        &key,
        TEXT_COLOR,
        236,
    );

    let action = fit_overlay_text(&mut font, row.action, fallback_scale, action_width);
    draw_overlay_text(
        font,
        frame,
        width,
        height,
        action_x.min(width.saturating_sub(1)),
        y.saturating_add(geometry.key_pad_y),
        fallback_scale,
        &action,
        TEXT_COLOR,
        228,
    );
}

fn draw_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    geometry: HelpGeometry,
    scroll_offset: usize,
    max_offset: usize,
) {
    let panel_right = (geometry.panel.x as u32).saturating_add(geometry.panel.width as u32);
    let track_x = panel_right
        .saturating_sub(geometry.pad_x / 2)
        .saturating_sub(geometry.scrollbar_width);
    let track_height = geometry.content_height.max(1);
    let visible = visible_line_count(geometry).max(1);
    let total = visible.saturating_add(max_offset).max(visible);
    let thumb_height = ((u64::from(track_height) * visible as u64) / total as u64)
        .max(8)
        .min(u64::from(track_height)) as u32;
    let thumb_range = track_height.saturating_sub(thumb_height);
    let thumb_y = geometry.content_y.saturating_add(
        (u64::from(thumb_range) * scroll_offset as u64 / max_offset.max(1) as u64) as u32,
    );
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(track_x),
            y: f64::from(thumb_y),
            width: f64::from(geometry.scrollbar_width),
            height: f64::from(thumb_height),
            radius: f64::from(geometry.scrollbar_width),
        },
        ACCENT_COLOR,
        232,
    );
}

fn help_geometry(
    width: u32,
    height: u32,
    text_size: u32,
    text_height: u32,
    fallback_scale: u32,
) -> HelpGeometry {
    let compact = text_size <= 12 || height < 260;
    let pad_x = if compact { 6 } else { 10 };
    let pad_y = if compact { 6 } else { (text_height / 2).max(8) };
    let inset_x = (width / 28).clamp(2, 78).min(width.saturating_sub(1) / 2);
    let inset_y = (height / 12).clamp(4, 58).min(height.saturating_sub(1) / 2);
    let max_panel_width = width.saturating_sub(inset_x.saturating_mul(2)).max(1);
    let max_panel_height = height.saturating_sub(inset_y.saturating_mul(2)).max(1);
    let key_pad_x = if compact { 4_u32 } else { 6 };
    let key_pad_y = if compact { 1_u32 } else { 3 };
    let scrollbar_width = 3;
    let wide_column_gap = if compact {
        pad_x
    } else {
        key_pad_x.saturating_mul(3) / 2
    };
    let two_column_width = help_panel_width_for_columns(
        2,
        fallback_scale,
        key_pad_x,
        pad_x,
        wide_column_gap,
        scrollbar_width,
    );
    let three_column_width = help_panel_width_for_columns(
        3,
        fallback_scale,
        key_pad_x,
        pad_x,
        wide_column_gap,
        scrollbar_width,
    );
    let column_count = if width >= 960 && height >= 150 && three_column_width <= max_panel_width {
        3
    } else if width >= 720 && height >= 150 && two_column_width <= max_panel_width {
        2
    } else {
        1
    };
    let column_gap = if column_count > 1 { wide_column_gap } else { 0 };
    let key_height = text_height.saturating_add(key_pad_y.saturating_mul(2));
    let line_pitch = key_height.saturating_add(if compact { 2 } else { 4 });
    let content_line_capacity = max_help_lines(column_count);
    let natural_content_height = line_pitch.saturating_mul(content_line_capacity as u32);
    let natural_height = pad_y
        .saturating_mul(2)
        .saturating_add(natural_content_height);
    let panel_height = natural_height.min(max_panel_height).max(1);
    let reserved_scrollbar = if natural_height > max_panel_height {
        scrollbar_width
    } else {
        0
    };
    let natural_width = help_panel_width_for_columns(
        column_count,
        fallback_scale,
        key_pad_x,
        pad_x,
        column_gap,
        reserved_scrollbar,
    );
    let panel_width = natural_width.min(max_panel_width).max(1);
    let inner_width = panel_width
        .saturating_sub(pad_x.saturating_mul(2))
        .saturating_sub(reserved_scrollbar)
        .max(1);
    let column_widths = if column_count > 1 {
        let available = inner_width
            .saturating_sub(column_gap.saturating_mul(column_count.saturating_sub(1) as u32));
        let natural_widths = help_column_content_widths(column_count, fallback_scale, key_pad_x);
        let natural_total = natural_widths.iter().sum::<u32>().max(1);
        if natural_total <= available {
            natural_widths
        } else {
            let mut widths = [0; MAX_HELP_COLUMNS];
            let even_width = available / column_count as u32;
            let mut assigned = 0_u32;
            for width in widths.iter_mut().take(column_count.saturating_sub(1)) {
                *width = even_width;
                assigned = assigned.saturating_add(even_width);
            }
            widths[column_count - 1] = available.saturating_sub(assigned);
            widths
        }
    } else {
        [inner_width, 0, 0]
    };
    let panel_x = width.saturating_sub(panel_width) / 2;
    let panel_y = height.saturating_sub(panel_height) / 2;
    let content_y = panel_y
        .saturating_add(pad_y)
        .min(panel_y.saturating_add(panel_height));
    let content_height = panel_y
        .saturating_add(panel_height)
        .saturating_sub(pad_y)
        .saturating_sub(content_y)
        .max(line_pitch.min(panel_height));

    HelpGeometry {
        panel: RoundedRect {
            x: f64::from(panel_x),
            y: f64::from(panel_y),
            width: f64::from(panel_width),
            height: f64::from(panel_height),
            radius: f64::from(rounded_radius(panel_width, panel_height, 8)),
        },
        pad_x,
        content_y,
        content_height,
        column_count,
        column_widths,
        column_gap,
        line_pitch,
        key_pad_x,
        key_pad_y,
        key_height,
        scrollbar_width: reserved_scrollbar,
    }
}

fn help_columns(column_count: usize) -> Vec<Vec<HelpLine>> {
    let column_count = column_count.clamp(1, MAX_HELP_COLUMNS);
    let mut columns = vec![Vec::new(); column_count];
    columns[0].push(HelpLine::Title("Active Controls"));
    for section in HELP_SECTIONS {
        let column = if column_count == 1 {
            0
        } else if column_count == 2 {
            match section.wide_column {
                HelpColumn::Left | HelpColumn::Info => 0,
                HelpColumn::Right => 1,
            }
        } else {
            match section.wide_column {
                HelpColumn::Left => 0,
                HelpColumn::Right => 1,
                HelpColumn::Info => 2,
            }
        };
        columns[column].push(HelpLine::Section(section.title));
        columns[column].extend(section.rows.iter().copied().map(HelpLine::Row));
    }
    columns
}

fn max_help_lines(column_count: usize) -> usize {
    help_columns(column_count)
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
}

fn help_panel_width_for_columns(
    column_count: usize,
    fallback_scale: u32,
    key_pad_x: u32,
    pad_x: u32,
    column_gap: u32,
    reserved_scrollbar: u32,
) -> u32 {
    pad_x
        .saturating_mul(2)
        .saturating_add(column_gap.saturating_mul(column_count.saturating_sub(1) as u32))
        .saturating_add(help_content_width(column_count, fallback_scale, key_pad_x))
        .saturating_add(reserved_scrollbar)
}

fn help_content_width(column_count: usize, fallback_scale: u32, key_pad_x: u32) -> u32 {
    help_column_content_widths(column_count, fallback_scale, key_pad_x)
        .iter()
        .sum()
}

fn help_column_content_widths(
    column_count: usize,
    fallback_scale: u32,
    key_pad_x: u32,
) -> [u32; MAX_HELP_COLUMNS] {
    let columns = help_columns(column_count);
    let column_gap = (key_pad_x / 2).max(6);
    let mut widths = [0; MAX_HELP_COLUMNS];
    for (index, width) in columns
        .iter()
        .map(|lines| {
            let heading_width = lines
                .iter()
                .filter_map(|line| match line {
                    HelpLine::Title(title) => Some(*title),
                    HelpLine::Section(title) => Some(*title),
                    HelpLine::Row(_) => None,
                })
                .map(|title| bitmap_text_width(title, fallback_scale))
                .max()
                .unwrap_or(0);
            let row_width = lines
                .iter()
                .filter_map(|line| match line {
                    HelpLine::Row(row) => Some(row),
                    HelpLine::Title(_) | HelpLine::Section(_) => None,
                })
                .map(|row| {
                    bitmap_text_width(row.key, fallback_scale)
                        .saturating_add(key_pad_x.saturating_mul(2))
                        .saturating_add(column_gap)
                        .saturating_add(bitmap_text_width(row.action, fallback_scale))
                })
                .max()
                .unwrap_or(0);
            heading_width.max(row_width)
        })
        .enumerate()
    {
        widths[index] = width;
    }
    widths
}

fn help_row_key_width(
    font: &mut Option<&mut FontRenderer>,
    row: HelpRow,
    fallback_scale: u32,
    geometry: HelpGeometry,
) -> u32 {
    overlay_text_width(font, row.key, fallback_scale)
        .saturating_add(geometry.key_pad_x.saturating_mul(2))
        .max(1)
}

fn visible_line_count(geometry: HelpGeometry) -> usize {
    (geometry.content_height / geometry.line_pitch).max(1) as usize
}

fn scroll_limit_for_columns(columns: &[Vec<HelpLine>], geometry: HelpGeometry) -> usize {
    let visible_lines = visible_line_count(geometry);
    columns
        .iter()
        .map(|column| column.len().saturating_sub(visible_lines))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_uses_one_column_on_narrow_canvases() {
        let geometry = help_geometry(240, 180, 12, 14, 1);

        assert_eq!(geometry.column_count, 1);
        assert!(help_scroll_limit(240, 180, 100, None) > 0);
    }

    #[test]
    fn help_uses_three_columns_when_space_allows() {
        let geometry = help_geometry(1280, 720, 18, 22, 2);

        assert_eq!(geometry.column_count, 3);
        assert!(
            geometry.panel.width as u32 <= 750,
            "panel width was {}",
            geometry.panel.width as u32
        );
        assert!(geometry.column_gap <= 12);
        assert!(geometry.column_widths[0] >= 260);
        assert!(geometry.column_widths[1] >= 200);
        assert!(geometry.column_widths[2] >= 150);
        assert!(geometry.panel.width as u32 <= 1000);
        assert_eq!(help_scroll_limit(1280, 720, 100, None), 0);
    }

    #[test]
    fn help_uses_two_columns_on_medium_canvases() {
        let geometry = help_geometry(800, 720, 18, 22, 2);

        assert_eq!(geometry.column_count, 2);
        assert!(
            geometry.panel.width as u32 <= 520,
            "panel width was {}",
            geometry.panel.width as u32
        );
        assert!(geometry.column_widths[0] >= 260);
        assert!(geometry.column_widths[1] >= 200);
        assert_eq!(help_scroll_limit(800, 720, 100, None), 0);
    }

    #[test]
    fn help_stacks_columns_when_two_columns_would_truncate() {
        let geometry = help_geometry(640, 360, 18, 22, 2);

        assert_eq!(geometry.column_count, 1);
        assert!(
            geometry.panel.width as u32 <= 290,
            "panel width was {}",
            geometry.panel.width as u32
        );
        assert!(geometry.column_widths[0] >= 240);
        assert!(help_scroll_limit(640, 360, 100, None) > 0);
    }

    #[test]
    fn help_single_column_stays_content_sized_on_small_canvases() {
        let geometry = help_geometry(360, 220, 12, 14, 1);

        assert_eq!(geometry.column_count, 1);
        assert!(geometry.panel.width as u32 <= 170);
        assert!(geometry.column_widths[0] >= 130);
    }

    #[test]
    fn help_does_not_sprawl_on_fullscreen_canvases() {
        let geometry = help_geometry(1920, 1080, 18, 22, 2);

        assert_eq!(geometry.column_count, 3);
        assert!(geometry.panel.width as u32 <= 1000);
    }

    #[test]
    fn help_title_lives_in_the_first_content_column() {
        let columns = help_columns(3);

        assert!(matches!(
            columns[0].first(),
            Some(HelpLine::Title("Active Controls"))
        ));
        assert!(!matches!(columns[1].first(), Some(HelpLine::Title(_))));
        assert!(!matches!(columns[2].first(), Some(HelpLine::Title(_))));
    }

    #[test]
    fn help_info_lives_in_third_column_when_available() {
        let columns = help_columns(3);

        assert!(
            columns[2]
                .iter()
                .any(|line| matches!(line, HelpLine::Section("Info")))
        );
        assert!(
            !columns[0]
                .iter()
                .any(|line| matches!(line, HelpLine::Section("Info")))
        );
        assert!(
            !columns[1]
                .iter()
                .any(|line| matches!(line, HelpLine::Section("Info")))
        );
    }

    #[test]
    fn help_scrollbar_matches_picker_weight() {
        let geometry = help_geometry(240, 180, 12, 14, 1);

        assert_eq!(geometry.scrollbar_width, 3);
    }

    #[test]
    fn help_model_keeps_case_sensitive_bindings() {
        let info = HELP_SECTIONS
            .iter()
            .find(|section| section.title == "Info")
            .expect("info section");

        assert!(info.rows.iter().any(|row| row.key == "i"));
        assert!(info.rows.iter().any(|row| row.key == "I"));
    }
}
