//! Top-level composition of a complete overlay frame.

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    controls::{
        draw_audio_control, draw_playback_control, draw_progress_handle, draw_subtitle_control,
        draw_track_picker,
    },
    layout::{OverlayMetrics, fallback_text_scale, rounded_radius, text_size},
    panels::{draw_media_info_panel, draw_top_message},
    raster::{RoundedRect, fill_rounded_rect},
    state::OverlayState,
    style::{ACCENT_COLOR, PANEL_COLOR, TEXT_COLOR, TRACK_COLOR},
    text::draw_overlay_text,
    timeline::{format_position_timestamp, format_timestamp, progress_pixels, time_column_width},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn render_overlay_rgb(
    frame: &mut [u8],
    width: u32,
    height: u32,
    terminal_rows: u16,
    scale_percent: u32,
    state: OverlayState,
    scratch: &mut String,
    acrylic: &mut AcrylicScratch,
    font: Option<&mut FontRenderer>,
) {
    if width == 0 || height == 0 || frame.len() < (width as usize * height as usize * 3) {
        return;
    }
    if !state.visible && state.status_message.is_none() && state.media_info.is_none() {
        return;
    }

    let text_size = text_size(width, height, scale_percent);
    let fallback_text_scale = fallback_text_scale(width, height, scale_percent);
    let mut font = font.and_then(|font| font.set_pixel_size(text_size).then_some(font));
    let text_height = font
        .as_ref()
        .map(|font| font.line_height())
        .unwrap_or(7 * fallback_text_scale);

    let title_visible =
        (state.visible || state.media_info.is_some()) && state.media_title.is_some();
    if title_visible && let Some(title) = state.media_title.as_deref() {
        draw_top_message(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            title,
            0,
            acrylic,
        );
    }

    if let Some(message) = state.status_message {
        draw_top_message(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            message,
            u32::from(title_visible),
            acrylic,
        );
    }

    if let Some(info) = state.media_info.as_ref() {
        draw_media_info_panel(
            font.as_deref_mut(),
            frame,
            width,
            height,
            text_size,
            fallback_text_scale,
            text_height,
            info,
            u32::from(title_visible) + u32::from(state.status_message.is_some()),
            acrylic,
        );
    }

    if !state.visible {
        return;
    }

    let time_width = time_column_width(font.as_deref_mut(), state.duration, fallback_text_scale);
    let metrics = OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        terminal_rows,
        time_width,
        state.audio_available,
        state.subtitles_available,
    );
    let panel_width = width
        .saturating_sub(metrics.inset_x.saturating_mul(2))
        .max(1);
    let panel_radius = rounded_radius(panel_width, metrics.panel_height, metrics.text_size);
    let panel_rect = RoundedRect {
        x: f64::from(metrics.inset_x),
        y: f64::from(metrics.panel_y),
        width: f64::from(panel_width),
        height: f64::from(metrics.panel_height),
        radius: f64::from(panel_radius),
    };
    fill_acrylic_rounded_rect(frame, width, height, panel_rect, PANEL_COLOR, 188, acrylic);

    let bar_radius = rounded_radius(
        metrics.bar_width,
        metrics.bar_height,
        metrics.bar_height / 2,
    );
    fill_rounded_rect(
        frame,
        width,
        height,
        RoundedRect {
            x: f64::from(metrics.bar_x),
            y: f64::from(metrics.bar_y),
            width: f64::from(metrics.bar_width),
            height: f64::from(metrics.bar_height),
            radius: f64::from(bar_radius),
        },
        TRACK_COLOR,
        218,
    );

    let filled = progress_pixels(metrics.bar_width, state.position, state.duration);
    if filled > 0 {
        fill_rounded_rect(
            frame,
            width,
            height,
            RoundedRect {
                x: f64::from(metrics.bar_x),
                y: f64::from(metrics.bar_y),
                width: f64::from(filled),
                height: f64::from(metrics.bar_height),
                radius: f64::from(rounded_radius(filled, metrics.bar_height, bar_radius)),
            },
            ACCENT_COLOR,
            248,
        );
    }
    if state.duration.is_some_and(|duration| !duration.is_zero()) {
        draw_progress_handle(frame, width, height, metrics, filled);
    }

    scratch.clear();
    scratch.push_str(&format_position_timestamp(state.position, state.duration));
    scratch.push_str(" / ");
    if let Some(duration) = state.duration {
        scratch.push_str(&format_timestamp(duration));
    } else {
        scratch.push_str("--:--");
    }

    draw_playback_control(frame, width, height, metrics, state.paused);
    if state.audio_available {
        draw_audio_control(
            frame,
            width,
            height,
            metrics,
            state.selected_audio.is_some(),
        );
        if state.audio_picker_open {
            draw_track_picker(
                font.as_deref_mut(),
                frame,
                width,
                height,
                metrics,
                &state.audio_labels,
                state.selected_audio,
                false,
                acrylic,
            );
        }
    }
    if state.subtitles_available {
        draw_subtitle_control(
            frame,
            width,
            height,
            metrics,
            state.selected_subtitle.is_some(),
        );
        if state.subtitle_picker_open {
            draw_track_picker(
                font.as_deref_mut(),
                frame,
                width,
                height,
                metrics,
                &state.subtitle_labels,
                state.selected_subtitle,
                true,
                acrylic,
            );
        }
    }
    draw_overlay_text(
        font,
        frame,
        width,
        height,
        metrics.time_x,
        metrics.text_y,
        metrics.fallback_text_scale,
        scratch,
        TEXT_COLOR,
        238,
    );
}
