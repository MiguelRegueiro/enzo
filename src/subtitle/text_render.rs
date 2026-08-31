use std::path::PathBuf;

use crate::font::{FontRenderer, ParagraphDirection, TextLayout};

const TEXT_COLOR: [u8; 3] = [255, 255, 255];
const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
pub(super) const MAX_SUBTITLE_WIDTH_RATIO: f64 = 0.84;
pub(super) const MAX_SUBTITLE_FALLBACK_FONTS: usize = 8;

#[cfg(test)]
#[path = "tests/text_render.rs"]
mod tests;

pub(super) struct CachedSubtitleLayout {
    pub(super) fallback_scale: u32,
    pub(super) lines: Vec<PreparedSubtitleLine>,
    pub(super) line_height: u32,
}

pub(super) struct PreparedSubtitleLine {
    pub(super) text: String,
    pub(super) width: u32,
    pub(super) layout: Option<TextLayout>,
}

pub(super) struct CachedTextOverlay {
    pub(super) canvas_width: u32,
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) premultiplied_rgba: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_text_overlay(
    mut font: Option<&mut FontRenderer>,
    canvas_width: u32,
    canvas_height: u32,
    start_y: u32,
    line_height: u32,
    line_gap: u32,
    fallback_scale: u32,
    lines: &[PreparedSubtitleLine],
) -> Option<CachedTextOverlay> {
    let block_height = line_height
        .saturating_mul(lines.len() as u32)
        .saturating_add(line_gap.saturating_mul(lines.len().saturating_sub(1) as u32));
    let band_padding = line_height.saturating_mul(2);
    let band_top = start_y.saturating_sub(band_padding);
    let band_bottom = start_y
        .saturating_add(block_height)
        .saturating_add(band_padding)
        .min(canvas_height);
    let band_height = band_bottom.checked_sub(band_top)?;
    let pixel_count = (canvas_width as usize).checked_mul(band_height as usize)?;
    let rgb_len = pixel_count.checked_mul(3)?;
    let mut over_black = vec![0_u8; rgb_len];
    let mut over_white = vec![255_u8; rgb_len];
    let mut y = start_y.saturating_sub(band_top);
    for line in lines {
        let x = canvas_width.saturating_sub(line.width) / 2;
        draw_prepared_subtitle_line(
            font.as_deref_mut(),
            &mut over_black,
            canvas_width,
            band_height,
            x,
            y,
            fallback_scale,
            line,
        );
        draw_prepared_subtitle_line(
            font.as_deref_mut(),
            &mut over_white,
            canvas_width,
            band_height,
            x,
            y,
            fallback_scale,
            line,
        );
        y = y.saturating_add(line_height).saturating_add(line_gap);
    }

    let mut left = canvas_width;
    let mut top = band_height;
    let mut right = 0_u32;
    let mut bottom = 0_u32;
    for pixel in 0..pixel_count {
        let offset = pixel * 3;
        let black = &over_black[offset..offset + 3];
        let white = &over_white[offset..offset + 3];
        let inverse_alpha = white[0]
            .saturating_sub(black[0])
            .max(white[1].saturating_sub(black[1]))
            .max(white[2].saturating_sub(black[2]));
        if inverse_alpha == 255 {
            continue;
        }
        let x = pixel as u32 % canvas_width;
        let y = pixel as u32 / canvas_width;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
    }
    let overlay_width = right.checked_sub(left)?;
    let overlay_height = bottom.checked_sub(top)?;
    let overlay_pixels = (overlay_width as usize).checked_mul(overlay_height as usize)?;
    let mut premultiplied_rgba = Vec::with_capacity(overlay_pixels.checked_mul(4)?);
    for row in top..bottom {
        for col in left..right {
            let offset = ((row * canvas_width + col) * 3) as usize;
            let black = &over_black[offset..offset + 3];
            let white = &over_white[offset..offset + 3];
            let inverse_alpha = white[0]
                .saturating_sub(black[0])
                .max(white[1].saturating_sub(black[1]))
                .max(white[2].saturating_sub(black[2]));
            premultiplied_rgba.extend_from_slice(&[black[0], black[1], black[2], inverse_alpha]);
        }
    }
    Some(CachedTextOverlay {
        canvas_width,
        x: left,
        y: band_top.saturating_add(top),
        width: overlay_width,
        height: overlay_height,
        premultiplied_rgba,
    })
}

pub(super) fn composite_text_overlay(frame: &mut [u8], overlay: &CachedTextOverlay) {
    for row in 0..overlay.height {
        for col in 0..overlay.width {
            let source_offset = ((row * overlay.width + col) * 4) as usize;
            let inverse_alpha = overlay.premultiplied_rgba[source_offset + 3];
            if inverse_alpha == 255 {
                continue;
            }
            let destination_offset =
                rgb_offset(overlay.canvas_width, overlay.x + col, overlay.y + row);
            for channel in 0..3 {
                let source = u16::from(overlay.premultiplied_rgba[source_offset + channel]);
                let destination = u16::from(frame[destination_offset + channel]);
                frame[destination_offset + channel] =
                    (source + (destination * u16::from(inverse_alpha) + 127) / 255).min(255) as u8;
            }
        }
    }
}

pub(super) fn open_first_font(paths: &[PathBuf], pixel_size: u32) -> Option<FontRenderer> {
    paths
        .iter()
        .find_map(|path| FontRenderer::open_path(path, pixel_size))
}

pub(super) fn prepare_subtitle_lines(
    lines: &[String],
    max_width: u32,
    fallback_scale: u32,
    mut font: Option<&mut FontRenderer>,
) -> Vec<PreparedSubtitleLine> {
    let mut out = Vec::new();
    for line in lines {
        if let Some(font) = font.as_deref_mut()
            && let Some(layout) = font.shape_text(line)
        {
            if layout.width() <= max_width {
                out.push(PreparedSubtitleLine {
                    text: line.clone(),
                    width: layout.width(),
                    layout: Some(layout),
                });
            } else {
                wrap_shaped_paragraph(font, line, max_width, layout.direction(), &mut out);
            }
            continue;
        }

        let mut current = String::new();
        for word in line.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if bitmap_text_width(&candidate, fallback_scale) <= max_width || current.is_empty() {
                current = candidate;
            } else {
                out.push(PreparedSubtitleLine {
                    width: bitmap_text_width(&current, fallback_scale),
                    text: current,
                    layout: None,
                });
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(PreparedSubtitleLine {
                width: bitmap_text_width(&current, fallback_scale),
                text: current,
                layout: None,
            });
        }
    }
    out
}

pub(super) fn wrap_shaped_paragraph(
    font: &mut FontRenderer,
    text: &str,
    max_width: u32,
    direction: ParagraphDirection,
    out: &mut Vec<PreparedSubtitleLine>,
) {
    let mut current = String::new();
    let mut current_layout = None;
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        let Some(candidate_layout) = font.shape_text_with_direction(&candidate, direction) else {
            continue;
        };
        if candidate_layout.width() <= max_width {
            current = candidate;
            current_layout = Some(candidate_layout);
            continue;
        }
        if let Some(layout) = current_layout.take() {
            out.push(PreparedSubtitleLine {
                text: std::mem::take(&mut current),
                width: layout.width(),
                layout: Some(layout),
            });
        }
        wrap_shaped_word(
            font,
            word,
            max_width,
            direction,
            out,
            &mut current,
            &mut current_layout,
        );
    }
    if let Some(layout) = current_layout {
        out.push(PreparedSubtitleLine {
            text: current,
            width: layout.width(),
            layout: Some(layout),
        });
    }
}

pub(super) fn wrap_shaped_word(
    font: &mut FontRenderer,
    word: &str,
    max_width: u32,
    direction: ParagraphDirection,
    out: &mut Vec<PreparedSubtitleLine>,
    current: &mut String,
    current_layout: &mut Option<TextLayout>,
) {
    let Some(word_layout) = font.shape_text_with_direction(word, direction) else {
        return;
    };
    if word_layout.width() <= max_width {
        *current = word.to_string();
        *current_layout = Some(word_layout);
        return;
    }

    let boundaries = word_layout.cluster_boundaries(word);
    let mut chunk_start = 0;
    let mut chunk_end = 0;
    let mut chunk_layout = None;
    for &boundary in boundaries.iter().skip(1) {
        let candidate = &word[chunk_start..boundary];
        let Some(candidate_layout) = font.shape_text_with_direction(candidate, direction) else {
            continue;
        };
        if candidate_layout.width() <= max_width || chunk_end == chunk_start {
            chunk_end = boundary;
            chunk_layout = Some(candidate_layout);
            continue;
        }
        if let Some(layout) = chunk_layout.take() {
            out.push(PreparedSubtitleLine {
                text: word[chunk_start..chunk_end].to_string(),
                width: layout.width(),
                layout: Some(layout),
            });
        }
        chunk_start = chunk_end;
        chunk_end = boundary;
        chunk_layout = font.shape_text_with_direction(&word[chunk_start..chunk_end], direction);
    }
    if let Some(layout) = chunk_layout {
        *current = word[chunk_start..chunk_end].to_string();
        *current_layout = Some(layout);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_prepared_subtitle_line(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    fallback_scale: u32,
    line: &PreparedSubtitleLine,
) {
    if let Some(font) = font.as_deref_mut()
        && let Some(layout) = line.layout.as_ref()
    {
        for (dx, dy) in [
            (-2, 0),
            (2, 0),
            (0, -2),
            (0, 2),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            font.draw_text_layout(
                frame,
                width,
                height,
                x as i32 + dx,
                y as i32 + dy,
                layout,
                SHADOW_COLOR,
                230,
            );
        }
        font.draw_text_layout(
            frame, width, height, x as i32, y as i32, layout, TEXT_COLOR, 255,
        );
        return;
    }
    for (dx, dy) in [
        (-2, 0),
        (2, 0),
        (0, -2),
        (0, 2),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ] {
        draw_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            x as i32 + dx,
            y as i32 + dy,
            fallback_scale,
            &line.text,
            SHADOW_COLOR,
            230,
        );
    }
    draw_text(
        font,
        frame,
        width,
        height,
        x as i32,
        y as i32,
        fallback_scale,
        &line.text,
        TEXT_COLOR,
        255,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_text(
    font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    fallback_scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    if let Some(font) = font {
        font.draw_text(frame, width, height, x, y, text, color, alpha);
    } else {
        draw_bitmap_text(
            frame,
            width,
            height,
            x.max(0) as u32,
            y.max(0) as u32,
            fallback_scale,
            text,
            color,
            alpha,
        );
    }
}

pub(super) fn subtitle_font_size(width: u32, height: u32) -> u32 {
    if width >= 960 && height >= 540 {
        34
    } else if width >= 420 && height >= 240 {
        26
    } else {
        16
    }
}

pub(super) fn fallback_text_scale(width: u32, height: u32) -> u32 {
    if width >= 960 && height >= 540 {
        4
    } else if width >= 420 && height >= 240 {
        3
    } else {
        2
    }
}

pub(super) fn subtitle_bottom_margin(height: u32) -> u32 {
    (height / 16).clamp(10, 46)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_bitmap_text(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    let scale = scale.max(1);
    let mut cursor = x;
    for ch in text.chars() {
        if let Some(glyph) = glyph(ch) {
            draw_glyph(frame, width, height, cursor, y, scale, glyph, color, alpha);
        }
        cursor = cursor.saturating_add(6 * scale);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_glyph(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    glyph: [u8; 7],
    color: [u8; 3],
    alpha: u8,
) {
    for (row, bits) in glyph.into_iter().enumerate() {
        for col in 0..5_u32 {
            if bits & (1_u8 << (4 - col)) == 0 {
                continue;
            }
            fill_solid_rect(
                frame,
                width,
                height,
                x + col * scale,
                y + row as u32 * scale,
                scale,
                scale,
                color,
                alpha,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fill_solid_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    cols: u32,
    rows: u32,
    color: [u8; 3],
    alpha: u8,
) {
    for py in y..y.saturating_add(rows).min(height) {
        for px in x..x.saturating_add(cols).min(width) {
            let offset = rgb_offset(width, px, py);
            blend_pixel(frame, offset, color, alpha);
        }
    }
}

pub(super) fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10011, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b11111, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b11111,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b11110, 0b00001, 0b00001, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b10010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01111, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b11110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '\'' => [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        ' ' => [0; 7],
        _ => return None,
    })
}

pub(super) fn bitmap_text_width(text: &str, scale: u32) -> u32 {
    let scale = scale.max(1);
    let chars = text.chars().count() as u32;
    if chars == 0 {
        0
    } else {
        chars * 6 * scale - scale
    }
}

pub(super) fn blend_pixel(frame: &mut [u8], offset: usize, color: [u8; 3], alpha: u8) {
    let inverse = u16::from(255 - alpha);
    let alpha = u16::from(alpha);
    for channel in 0..3 {
        let source = u16::from(color[channel]) * alpha;
        let dest = u16::from(frame[offset + channel]) * inverse;
        frame[offset + channel] = ((source + dest + 127) / 255) as u8;
    }
}

pub(super) fn rgb_offset(width: u32, x: u32, y: u32) -> usize {
    ((y * width + x) * 3) as usize
}
