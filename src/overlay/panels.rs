//! Title, status, and media-information panel drawing.

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    layout::{
        horizontal_padding_for_text, rounded_radius, top_message_stack_y, vertical_padding_for_text,
    },
    raster::RoundedRect,
    state::MediaInfoState,
    style::{PANEL_COLOR, TEXT_COLOR},
    text::{bitmap_text_width, draw_overlay_text},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_top_message(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    text: &str,
    stack_index: u32,
    acrylic: &mut AcrylicScratch,
) {
    let inset_x = (width / 48).clamp(8, 34).min(width.saturating_sub(1));
    let inset_y = top_message_stack_y(height, text_size, text_height, stack_index);
    let pad_x = (horizontal_padding_for_text(text_size) / 2).max(6);
    let pad_y = (vertical_padding_for_text(text_size) / 2).max(4);
    let natural_panel_height = text_height.saturating_add(pad_y.saturating_mul(2));
    let text_width = font
        .as_mut()
        .map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, fallback_scale));
    let panel_width = text_width
        .saturating_add(pad_x.saturating_mul(2))
        .min(width.saturating_sub(inset_x).max(1));
    let panel_height = natural_panel_height.min(height.saturating_sub(inset_y).max(1));
    let panel_x = inset_x;
    let panel_radius = rounded_radius(panel_width, panel_height, text_size / 3);

    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(panel_x),
            y: f64::from(inset_y),
            width: f64::from(panel_width),
            height: f64::from(panel_height),
            radius: f64::from(panel_radius),
        },
        PANEL_COLOR,
        202,
        acrylic,
    );

    draw_overlay_text(
        font,
        frame,
        width,
        height,
        panel_x.saturating_add(pad_x).min(width.saturating_sub(1)),
        inset_y.saturating_add(pad_y).min(height.saturating_sub(1)),
        fallback_scale,
        text,
        TEXT_COLOR,
        244,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_media_info_panel(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    text_size: u32,
    fallback_scale: u32,
    text_height: u32,
    info: &MediaInfoState,
    stack_index: u32,
    acrylic: &mut AcrylicScratch,
) {
    let mut rows = vec![
        ("FILE", info.info.file.to_string()),
        ("DISPLAY", display_info_text(info)),
        ("VIDEO", info.info.video.to_string()),
    ];
    if !info.info.audio.is_empty() {
        let audio = info
            .selected_audio
            .and_then(|index| info.info.audio.get(index))
            .map_or_else(|| "None".to_string(), ToString::to_string);
        rows.push(("AUDIO", audio));
    }

    let inset_x = (width / 48).clamp(8, 34).min(width.saturating_sub(1));
    let inset_y = top_message_stack_y(height, text_size, text_height, stack_index);
    let pad_x = (horizontal_padding_for_text(text_size) / 2).max(6);
    let pad_y = (vertical_padding_for_text(text_size) / 2).max(4);
    let column_gap = pad_x;
    let row_gap = (pad_y / 2).max(2);
    let max_panel_width = width.saturating_sub(inset_x.saturating_mul(2)).max(1);
    let label_width = rows
        .iter()
        .map(|(label, _)| overlay_text_width(&mut font, label, fallback_scale))
        .max()
        .unwrap_or(0);
    let max_value_width = max_panel_width
        .saturating_sub(pad_x.saturating_mul(2))
        .saturating_sub(label_width)
        .saturating_sub(column_gap)
        .max(1);
    for (_, value) in &mut rows {
        *value = fit_overlay_text(&mut font, value, fallback_scale, max_value_width);
    }
    let value_width = rows
        .iter()
        .map(|(_, value)| overlay_text_width(&mut font, value, fallback_scale))
        .max()
        .unwrap_or(0);
    let panel_width = pad_x
        .saturating_mul(2)
        .saturating_add(label_width)
        .saturating_add(column_gap)
        .saturating_add(value_width)
        .min(max_panel_width);
    let content_height = text_height
        .saturating_mul(rows.len() as u32)
        .saturating_add(row_gap.saturating_mul(rows.len().saturating_sub(1) as u32));
    let panel_height = content_height
        .saturating_add(pad_y.saturating_mul(2))
        .min(height.saturating_sub(inset_y).max(1));
    let panel_x = inset_x;
    let radius = rounded_radius(panel_width, panel_height, text_size / 3);

    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(panel_x),
            y: f64::from(inset_y),
            width: f64::from(panel_width),
            height: f64::from(panel_height),
            radius: f64::from(radius),
        },
        PANEL_COLOR,
        202,
        acrylic,
    );

    let value_x = panel_x
        .saturating_add(pad_x)
        .saturating_add(label_width)
        .saturating_add(column_gap);
    for (index, (label, value)) in rows.iter().enumerate() {
        let y = inset_y
            .saturating_add(pad_y)
            .saturating_add((text_height + row_gap).saturating_mul(index as u32));
        draw_overlay_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            panel_x.saturating_add(pad_x),
            y,
            fallback_scale,
            label,
            TEXT_COLOR,
            174,
        );
        draw_overlay_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            value_x,
            y,
            fallback_scale,
            value,
            TEXT_COLOR,
            244,
        );
    }
}

fn display_info_text(info: &MediaInfoState) -> String {
    let output = format!("Kitty · {}×{}", info.display_width, info.display_height);
    if info.display_paused {
        return format!("{output} · paused");
    }
    info.display_fps
        .map_or(output.clone(), |fps| format!("{output} · {fps:.1} fps"))
}

fn fit_overlay_text(
    font: &mut Option<&mut FontRenderer>,
    text: &str,
    fallback_scale: u32,
    max_width: u32,
) -> String {
    if overlay_text_width(font, text, fallback_scale) <= max_width {
        return text.to_string();
    }

    let suffix = "...";
    if overlay_text_width(font, suffix, fallback_scale) > max_width {
        return suffix.to_string();
    }

    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let mut fits_through = 0_usize;
    let mut does_not_fit_from = boundaries.len().saturating_sub(1);
    while fits_through < does_not_fit_from {
        let candidate_chars = fits_through + (does_not_fit_from - fits_through).div_ceil(2);
        let candidate = format!("{}{suffix}", &text[..boundaries[candidate_chars]]);
        if overlay_text_width(font, &candidate, fallback_scale) <= max_width {
            fits_through = candidate_chars;
        } else {
            does_not_fit_from = candidate_chars.saturating_sub(1);
        }
    }

    format!("{}{suffix}", &text[..boundaries[fits_through]])
}

fn overlay_text_width(
    font: &mut Option<&mut FontRenderer>,
    text: &str,
    fallback_scale: u32,
) -> u32 {
    font.as_deref_mut()
        .map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, fallback_scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{
        state::{MediaInfo, MediaInfoState},
        text::bitmap_text_width,
    };

    #[test]
    fn display_info_describes_output_backend_size_and_measured_rate() {
        let playing = MediaInfoState {
            info: MediaInfo::new(String::new(), String::new(), Vec::new()),
            selected_audio: None,
            display_width: 1280,
            display_height: 720,
            display_paused: false,
            display_fps: Some(29.8),
        };

        assert_eq!(display_info_text(&playing), "Kitty · 1280×720 · 29.8 fps");

        let paused = MediaInfoState {
            display_paused: true,
            ..playing
        };
        assert_eq!(display_info_text(&paused), "Kitty · 1280×720 · paused");
    }

    #[test]
    fn overlay_text_truncation_keeps_the_longest_fitting_prefix() {
        let mut font = None;
        let max_width = bitmap_text_width("ABCDE...", 1);

        assert_eq!(
            fit_overlay_text(&mut font, "ABCDEFGHIJ", 1, max_width),
            "ABCDE..."
        );
        assert_eq!(
            fit_overlay_text(&mut font, "éééééééééé", 1, max_width),
            "ééééé..."
        );
        assert_eq!(fit_overlay_text(&mut font, "ABCDE", 1, max_width), "ABCDE");
        assert_eq!(fit_overlay_text(&mut font, "ABCDE", 1, 0), "...");
    }
}
