//! Compact options panel with shared drawing and hit-test geometry.

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    geometry::{fallback_text_scale, text_size},
    raster::{
        Circle, RoundedRect, fill_circle, fill_rounded_rect, fill_solid_rect, stroke_rounded_rect,
    },
    state::{HitboxRect, OverlayHitPoint, OverlayRenderContext},
    style::{PANEL_COLOR, TEXT_COLOR, TRACK_COLOR},
    text::{draw_overlay_text, fit_overlay_text, overlay_text_width},
};

#[derive(Clone, Debug)]
pub(crate) struct OptionsMenuState {
    pub(crate) name: &'static str,
    pub(crate) position: usize,
    pub(crate) count: usize,
    pub(crate) color: [u8; 3],
    pub(crate) editor: Option<HexInputState>,
    pub(crate) selected_setting: OptionsSetting,
    pub(crate) panel_opacity: u8,
    pub(crate) background_blur: u8,
    pub(crate) playback_controls_autohide: u16,
    pub(crate) seconds_editor: Option<SecondsInputState>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OptionsSetting {
    AccentColor,
    PanelOpacity,
    BackgroundBlur,
    PlaybackControlsAutohide,
}

impl OptionsSetting {
    pub(crate) fn navigate(self, direction: i32) -> Self {
        const SETTINGS: [OptionsSetting; 4] = [
            OptionsSetting::AccentColor,
            OptionsSetting::PanelOpacity,
            OptionsSetting::BackgroundBlur,
            OptionsSetting::PlaybackControlsAutohide,
        ];
        let index = match self {
            Self::AccentColor => 0,
            Self::PanelOpacity => 1,
            Self::BackgroundBlur => 2,
            Self::PlaybackControlsAutohide => 3,
        };
        SETTINGS[(index as i32 + direction).rem_euclid(SETTINGS.len() as i32) as usize]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HexInputState {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) focused: bool,
    pub(crate) selected: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SecondsInputState {
    pub(crate) text: String,
    pub(crate) focused: bool,
    pub(crate) selected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OptionsAction {
    Cycle(i32),
    CycleAccent(i32),
    AdjustOpacity(i32),
    SetOpacity(u8),
    AdjustBlur(i32),
    SetBlur(u8),
    CycleAutohide(i32),
    SecondsCursor,
    Cursor(usize),
    Close,
}

struct OptionsGeometry {
    panel: HitboxRect,
    pad: u32,
    pitch: u32,
    text_height: u32,
    scale: u32,
}

impl OptionsGeometry {
    fn row(&self, index: u32) -> HitboxRect {
        let top = self.panel.top + self.pad + index * self.pitch;
        HitboxRect {
            left: self.panel.left + self.pad,
            right: self.panel.right.saturating_sub(self.pad),
            top,
            bottom: (top + self.pitch).min(self.panel.bottom.saturating_sub(self.pad)),
        }
    }

    fn arrow(&self, row_index: u32, direction: i32) -> HitboxRect {
        let row = self.row(row_index);
        let size = (self.text_height + self.pad).min(row.right.saturating_sub(row.left) / 3);
        let top = row.top + row.bottom.saturating_sub(row.top).saturating_sub(size) / 2;
        let bottom = top + size;
        if direction < 0 {
            HitboxRect {
                right: row.left + size,
                top,
                bottom,
                ..row
            }
        } else {
            HitboxRect {
                left: row.right.saturating_sub(size),
                top,
                bottom,
                ..row
            }
        }
    }

    fn editor_rect(&self, name: &str, font: &mut Option<&mut FontRenderer>) -> HitboxRect {
        let row = self.row(2);
        HitboxRect {
            left: self.arrow(2, -1).right
                + self.text_height
                + self.pad * 3
                + overlay_text_width(font, name, self.scale),
            right: self.arrow(2, 1).left.saturating_sub(self.pad / 2),
            ..row
        }
    }
}

fn geometry(
    context: OverlayRenderContext,
    state: &OptionsMenuState,
    font: &mut Option<&mut FontRenderer>,
) -> OptionsGeometry {
    let size = text_size(context.width, context.height, context.scale_percent);
    let scale = fallback_text_scale(context.width, context.height, context.scale_percent);
    if let Some(font) = font.as_deref_mut() {
        font.set_pixel_size(size);
    }
    let text_height = font.as_ref().map_or(7 * scale, |font| font.line_height());
    let pad = (size / 2).max(4);
    let pitch = text_height.max(size) + pad * 2;
    let width = (size * 18 + pad * 2)
        .max(overlay_text_width(font, "Custom#ffffff", scale) + pitch * 2 + text_height + pad * 6)
        .min(context.width.saturating_sub(8));
    let rows = 9 + u32::from(state.error.is_some());
    let height = (pitch * rows + pad * 2).min(context.height.saturating_sub(8));
    let left = context.width.saturating_sub(width) / 2;
    let top = context.height.saturating_sub(height) / 2;
    OptionsGeometry {
        panel: HitboxRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
        pad,
        pitch,
        text_height,
        scale,
    }
}

fn contains(rect: HitboxRect, point: OverlayHitPoint) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

pub(super) fn options_action(
    context: OverlayRenderContext,
    state: &OptionsMenuState,
    point: OverlayHitPoint,
    mut font: Option<&mut FontRenderer>,
) -> Option<OptionsAction> {
    let geometry = geometry(context, state, &mut font);
    if !contains(geometry.panel, point) {
        return Some(OptionsAction::Close);
    }
    for direction in [-1, 1] {
        if contains(geometry.arrow(2, direction), point) {
            return Some(OptionsAction::CycleAccent(direction));
        }
        if contains(geometry.arrow(4, direction), point) {
            return Some(OptionsAction::AdjustOpacity(direction));
        }
    }
    let opacity_track = value_track(&geometry, 4);
    if contains(opacity_track, point) {
        let width = opacity_track
            .right
            .saturating_sub(opacity_track.left)
            .max(1);
        let offset = point.x.saturating_sub(opacity_track.left).min(width);
        return Some(OptionsAction::SetOpacity(((offset * 100) / width) as u8));
    }
    for direction in [-1, 1] {
        if contains(geometry.arrow(6, direction), point) {
            return Some(OptionsAction::AdjustBlur(direction));
        }
        if contains(geometry.arrow(8, direction), point) {
            return Some(OptionsAction::CycleAutohide(direction));
        }
    }
    let blur_track = value_track(&geometry, 6);
    if contains(blur_track, point) {
        let width = blur_track.right.saturating_sub(blur_track.left).max(1);
        let offset = point.x.saturating_sub(blur_track.left).min(width);
        return Some(OptionsAction::SetBlur(((offset * 24) / width) as u8));
    }
    if state.seconds_editor.is_some() && contains(geometry.row(8), point) {
        return Some(OptionsAction::SecondsCursor);
    }
    if let Some(editor) = &state.editor {
        let row = geometry.editor_rect(state.name, &mut font);
        if contains(row, point) {
            let x = point.x.saturating_sub(row.left);
            for index in 1..editor.text.len() {
                let left = overlay_text_width(&mut font, &editor.text[..index], geometry.scale);
                let right =
                    overlay_text_width(&mut font, &editor.text[..index + 1], geometry.scale);
                if x < (left + right) / 2 {
                    return Some(OptionsAction::Cursor(index));
                }
            }
            return Some(OptionsAction::Cursor(editor.text.len()));
        }
    }
    None
}

fn value_track(geometry: &OptionsGeometry, row_index: u32) -> HitboxRect {
    let row = geometry.row(row_index);
    HitboxRect {
        left: geometry
            .arrow(row_index, -1)
            .right
            .saturating_add(geometry.pad),
        right: geometry
            .arrow(row_index, 1)
            .left
            .saturating_sub(geometry.pad),
        ..row
    }
}

fn rounded(rect: HitboxRect) -> RoundedRect {
    RoundedRect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(rect.right.saturating_sub(rect.left)),
        height: f64::from(rect.bottom.saturating_sub(rect.top)),
        radius: 5.0,
    }
}

pub(super) fn draw_options_menu(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    context: OverlayRenderContext,
    state: &OptionsMenuState,
    panel_alpha: u8,
    blur_radius: u32,
    acrylic: &mut AcrylicScratch,
) {
    let geometry = geometry(context, state, &mut font);
    let width = context.width;
    let height = context.height;
    fill_acrylic_rounded_rect(
        frame,
        width,
        height,
        rounded(geometry.panel),
        PANEL_COLOR,
        panel_alpha,
        blur_radius,
        acrylic,
    );
    let accent_label = format!("Accent color  {}/{}", state.position, state.count);
    let opacity_label = format!("Panel opacity  {}%", state.panel_opacity);
    let blur_label = format!("Background blur  {}px", state.background_blur);
    let autohide_label = "Playback controls auto-hide";
    let autohide_value = state.seconds_editor.as_ref().map_or_else(
        || {
            if autohide_choice(state.playback_controls_autohide) == 4 {
                format!("Custom {}s", state.playback_controls_autohide)
            } else {
                autohide_name(state.playback_controls_autohide).to_owned()
            }
        },
        |_| "Custom".to_owned(),
    );
    let mut rows = vec![
        "Options",
        &accent_label,
        state.name,
        &opacity_label,
        "",
        &blur_label,
        "",
        autohide_label,
        &autohide_value,
    ];
    if let Some(error) = &state.error {
        rows.push(error);
    }
    let selected_row = match state.selected_setting {
        OptionsSetting::AccentColor => 1,
        OptionsSetting::PanelOpacity => 3,
        OptionsSetting::BackgroundBlur => 5,
        OptionsSetting::PlaybackControlsAutohide => 7,
    };
    let control = geometry.row(selected_row + 1);
    let focus = HitboxRect {
        left: geometry
            .arrow(selected_row + 1, -1)
            .right
            .saturating_add(geometry.pad / 2),
        right: geometry
            .arrow(selected_row + 1, 1)
            .left
            .saturating_sub(geometry.pad / 2),
        top: control.top.saturating_add(geometry.pad / 4),
        bottom: control.bottom.saturating_sub(geometry.pad / 4),
    };
    fill_rounded_rect(frame, width, height, rounded(focus), TEXT_COLOR, 32);
    for (index, text) in rows.into_iter().enumerate() {
        let row = geometry.row(index as u32);
        if row.bottom.saturating_sub(row.top) < geometry.text_height {
            continue;
        }
        let y = row.top + (geometry.pitch - geometry.text_height) / 2;
        let mut x = row.left + geometry.pad;
        let mut right = row.right.saturating_sub(geometry.pad);
        if index == 2 {
            for (direction, symbol) in [(-1, "<"), (1, ">")] {
                let button = geometry.arrow(2, direction);
                fill_rounded_rect(frame, width, height, rounded(button), TRACK_COLOR, 100);
                let symbol_width = overlay_text_width(&mut font, symbol, geometry.scale);
                let symbol_x = button.left
                    + (button
                        .right
                        .saturating_sub(button.left)
                        .saturating_sub(symbol_width))
                        / 2;
                draw_overlay_text(
                    font.as_deref_mut(),
                    frame,
                    width,
                    height,
                    symbol_x,
                    y,
                    geometry.scale,
                    symbol,
                    TEXT_COLOR,
                    248,
                );
            }
            x = geometry.arrow(2, -1).right + geometry.pad;
            right = geometry.arrow(2, 1).left.saturating_sub(geometry.pad);
            let swatch = HitboxRect {
                left: x,
                right: (x + geometry.text_height).min(right),
                top: y,
                bottom: y + geometry.text_height,
            };
            let swatch_color = state.editor.as_ref().map_or(state.color, |editor| {
                crate::config::parse_hex_color(&editor.text).unwrap_or([0, 0, 0])
            });
            fill_rounded_rect(frame, width, height, rounded(swatch), swatch_color, 255);
            x += geometry.text_height + geometry.pad;
        }
        if index == 8 {
            x = geometry.arrow(8, -1).right + geometry.pad;
            right = geometry.arrow(8, 1).left.saturating_sub(geometry.pad);
        }
        let fitted = fit_overlay_text(&mut font, text, geometry.scale, right.saturating_sub(x));
        draw_overlay_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            x,
            y,
            geometry.scale,
            &fitted,
            match index {
                1 if state.selected_setting == OptionsSetting::AccentColor => state.color,
                3 if state.selected_setting == OptionsSetting::PanelOpacity => state.color,
                5 if state.selected_setting == OptionsSetting::BackgroundBlur => state.color,
                7 if state.selected_setting == OptionsSetting::PlaybackControlsAutohide => {
                    state.color
                }
                _ => TEXT_COLOR,
            },
            248,
        );
        if let Some((value, maximum)) = match index {
            4 => Some((state.panel_opacity, 100)),
            6 => Some((state.background_blur, 24)),
            8 => Some((0, 1)),
            _ => None,
        } {
            for (direction, symbol) in [(-1, "<"), (1, ">")] {
                let button = geometry.arrow(index as u32, direction);
                fill_rounded_rect(frame, width, height, rounded(button), TRACK_COLOR, 100);
                let symbol_width = overlay_text_width(&mut font, symbol, geometry.scale);
                let symbol_x = button.left
                    + (button
                        .right
                        .saturating_sub(button.left)
                        .saturating_sub(symbol_width))
                        / 2;
                draw_overlay_text(
                    font.as_deref_mut(),
                    frame,
                    width,
                    height,
                    symbol_x,
                    y,
                    geometry.scale,
                    symbol,
                    TEXT_COLOR,
                    248,
                );
            }
            if index == 8 {
                if let Some(editor) = &state.seconds_editor {
                    let field_left = x
                        .saturating_add(overlay_text_width(&mut font, "Custom", geometry.scale))
                        .saturating_add(geometry.pad);
                    let field = HitboxRect {
                        left: field_left,
                        right,
                        top: geometry.arrow(8, -1).top,
                        bottom: geometry.arrow(8, -1).bottom,
                    };
                    fill_rounded_rect(frame, width, height, rounded(field), TRACK_COLOR, 60);
                    if state.selected_setting == OptionsSetting::PlaybackControlsAutohide {
                        stroke_rounded_rect(
                            frame,
                            width,
                            height,
                            rounded(field),
                            1.0,
                            state.color,
                            220,
                        );
                    }
                    let value = if editor.text.is_empty() {
                        "1–9999s".to_owned()
                    } else {
                        format!("{}s", editor.text)
                    };
                    let fitted = fit_overlay_text(
                        &mut font,
                        &value,
                        geometry.scale,
                        field.right.saturating_sub(field.left),
                    );
                    draw_overlay_text(
                        font.as_deref_mut(),
                        frame,
                        width,
                        height,
                        field.left,
                        y,
                        geometry.scale,
                        &fitted,
                        TEXT_COLOR,
                        if editor.text.is_empty() { 115 } else { 248 },
                    );
                    let cursor_x =
                        field.left + overlay_text_width(&mut font, &editor.text, geometry.scale);
                    if editor.focused && cursor_x < field.right {
                        fill_solid_rect(
                            frame,
                            width,
                            height,
                            cursor_x,
                            y,
                            1,
                            geometry.text_height,
                            TEXT_COLOR,
                            255,
                        );
                    }
                }
                continue;
            }
            let track = value_track(&geometry, index as u32);
            const SLIDER_HEIGHT: u32 = 5;
            const SLIDER_HANDLE_RADIUS: f64 = 6.0;
            let track_y = y + geometry.text_height.saturating_sub(SLIDER_HEIGHT) / 2;
            fill_rounded_rect(
                frame,
                width,
                height,
                RoundedRect {
                    x: f64::from(track.left),
                    y: f64::from(track_y),
                    width: f64::from(track.right.saturating_sub(track.left)),
                    height: f64::from(SLIDER_HEIGHT),
                    radius: 2.0,
                },
                TRACK_COLOR,
                180,
            );
            let filled =
                track.left + track.right.saturating_sub(track.left) * u32::from(value) / maximum;
            let slider_focused = matches!(
                (index, state.selected_setting),
                (4, OptionsSetting::PanelOpacity) | (6, OptionsSetting::BackgroundBlur)
            );
            fill_rounded_rect(
                frame,
                width,
                height,
                RoundedRect {
                    x: f64::from(track.left),
                    y: f64::from(track_y),
                    width: f64::from(filled.saturating_sub(track.left)),
                    height: f64::from(SLIDER_HEIGHT),
                    radius: 2.0,
                },
                if slider_focused {
                    state.color
                } else {
                    TEXT_COLOR
                },
                if slider_focused { 248 } else { 72 },
            );
            if slider_focused {
                fill_circle(
                    frame,
                    width,
                    height,
                    Circle {
                        x: f64::from(filled),
                        y: f64::from(track_y) + f64::from(SLIDER_HEIGHT) / 2.0,
                        radius: SLIDER_HANDLE_RADIUS,
                    },
                    state.color,
                    255,
                );
            }
        }
        if index == 2
            && let Some(editor) = &state.editor
        {
            let field = HitboxRect {
                top: geometry.arrow(2, -1).top,
                bottom: geometry.arrow(2, -1).bottom,
                ..geometry.editor_rect(state.name, &mut font)
            };
            let empty = editor.text.len() == 1;
            let background = HitboxRect {
                left: field.left.saturating_sub(geometry.pad / 2),
                ..field
            };
            fill_rounded_rect(frame, width, height, rounded(background), TRACK_COLOR, 60);
            if state.selected_setting == OptionsSetting::AccentColor {
                stroke_rounded_rect(
                    frame,
                    width,
                    height,
                    rounded(background),
                    1.0,
                    state.color,
                    220,
                );
            }
            let fitted = fit_overlay_text(
                &mut font,
                if empty { "#RRGGBB" } else { &editor.text },
                geometry.scale,
                field.right.saturating_sub(field.left),
            );
            draw_overlay_text(
                font.as_deref_mut(),
                frame,
                width,
                height,
                field.left,
                y,
                geometry.scale,
                &fitted,
                TEXT_COLOR,
                if empty { 115 } else { 248 },
            );
            let cursor_x = field.left
                + overlay_text_width(&mut font, &editor.text[..editor.cursor], geometry.scale);
            if (editor.focused || state.selected_setting == OptionsSetting::AccentColor)
                && cursor_x < field.right
            {
                fill_solid_rect(
                    frame,
                    width,
                    height,
                    cursor_x,
                    y,
                    1,
                    geometry.text_height,
                    TEXT_COLOR,
                    255,
                );
            }
        }
    }
}

pub(crate) fn autohide_choice(seconds: u16) -> usize {
    match seconds {
        2 => 0,
        4 => 1,
        8 => 2,
        0 => 3,
        _ => 4,
    }
}

pub(crate) fn autohide_name(seconds: u16) -> &'static str {
    ["2s", "4s", "8s", "Never", "Custom"][autohide_choice(seconds)]
}

#[cfg(test)]
#[path = "tests/options_overlay.rs"]
mod tests;
