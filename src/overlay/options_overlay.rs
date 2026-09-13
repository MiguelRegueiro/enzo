//! Compact options panel with shared drawing and hit-test geometry.

use crate::font::FontRenderer;

use super::{
    acrylic::{AcrylicScratch, fill_acrylic_rounded_rect},
    geometry::{fallback_text_scale, text_size},
    raster::{RoundedRect, fill_rounded_rect, fill_solid_rect},
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
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct HexInputState {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) focused: bool,
    pub(crate) selected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OptionsAction {
    Cycle(i32),
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

    fn arrow(&self, direction: i32) -> HitboxRect {
        let row = self.row(2);
        let size = self.pitch.min(row.right.saturating_sub(row.left) / 3);
        if direction < 0 {
            HitboxRect {
                right: row.left + size,
                ..row
            }
        } else {
            HitboxRect {
                left: row.right.saturating_sub(size),
                ..row
            }
        }
    }

    fn editor_rect(&self, name: &str, font: &mut Option<&mut FontRenderer>) -> HitboxRect {
        let row = self.row(2);
        HitboxRect {
            left: self.arrow(-1).right
                + self.text_height
                + self.pad * 3
                + overlay_text_width(font, name, self.scale),
            right: self.arrow(1).left.saturating_sub(self.pad),
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
    let pitch = text_height.max(size) + pad;
    let width = (size * 16 + pad * 2)
        .max(overlay_text_width(font, "Custom#ffffff", scale) + pitch * 2 + text_height + pad * 6)
        .min(context.width.saturating_sub(8));
    let rows = 3 + u32::from(state.error.is_some());
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
        if contains(geometry.arrow(direction), point) {
            return Some(OptionsAction::Cycle(direction));
        }
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
        224,
        acrylic,
    );
    let accent_label = format!("Accent color  {}/{}", state.position, state.count);
    let mut rows = vec!["Options", &accent_label, state.name];
    if let Some(error) = &state.error {
        rows.push(error);
    }
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
                let button = geometry.arrow(direction);
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
            x = geometry.arrow(-1).right + geometry.pad;
            right = geometry.arrow(1).left.saturating_sub(geometry.pad);
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
            if index == 1 { state.color } else { TEXT_COLOR },
            248,
        );
        if index == 2
            && let Some(editor) = &state.editor
        {
            let field = geometry.editor_rect(state.name, &mut font);
            let empty = editor.text.len() == 1;
            let background = HitboxRect {
                left: field.left.saturating_sub(geometry.pad / 2),
                ..field
            };
            fill_rounded_rect(frame, width, height, rounded(background), TRACK_COLOR, 60);
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
    }
}

#[cfg(test)]
#[path = "tests/options_overlay.rs"]
mod tests;
