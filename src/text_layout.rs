use std::{ffi::CString, ffi::c_void, mem, slice};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PositionedGlyph {
    pub(crate) index: u32,
    pub(crate) font_index: u32,
    pub(crate) cluster: u32,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct TextLayout {
    glyphs: Vec<PositionedGlyph>,
    width: u32,
    direction: ParagraphDirection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParagraphDirection {
    Auto = -1,
    LeftToRight = 0,
    RightToLeft = 1,
}

impl TextLayout {
    pub(crate) fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn direction(&self) -> ParagraphDirection {
        self.direction
    }

    pub(crate) fn cluster_boundaries(&self, text: &str) -> Vec<usize> {
        let mut codepoint_starts = self
            .glyphs
            .iter()
            .map(|glyph| glyph.cluster as usize)
            .collect::<Vec<_>>();
        codepoint_starts.push(0);
        codepoint_starts.sort_unstable();
        codepoint_starts.dedup();

        let mut boundaries = text
            .char_indices()
            .enumerate()
            .filter_map(|(codepoint, (byte, _))| {
                codepoint_starts
                    .binary_search(&codepoint)
                    .is_ok()
                    .then_some(byte)
            })
            .collect::<Vec<_>>();
        boundaries.push(text.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
    }
}

#[repr(C)]
struct EnzoShapedGlyph {
    glyph_index: u32,
    font_index: u32,
    cluster: u32,
    x_advance: i32,
    x_offset: i32,
    y_offset: i32,
}

#[repr(C)]
#[derive(Default)]
struct EnzoShapedText {
    glyphs: *mut EnzoShapedGlyph,
    count: usize,
    paragraph_rtl: i32,
}

unsafe extern "C" {
    fn enzo_shape_text(
        freetype_faces: *const *mut c_void,
        face_count: usize,
        utf8: *const std::ffi::c_char,
        paragraph_direction: i32,
        out: *mut EnzoShapedText,
    ) -> std::ffi::c_int;
    fn enzo_shaped_text_free(text: *mut EnzoShapedText);
}

pub(crate) fn shape_with_direction(
    faces: &[*mut c_void],
    text: &str,
    direction: ParagraphDirection,
) -> Option<TextLayout> {
    if faces.is_empty() {
        return None;
    }
    let text = CString::new(text).ok()?;
    let mut shaped = EnzoShapedText::default();
    if unsafe {
        enzo_shape_text(
            faces.as_ptr(),
            faces.len(),
            text.as_ptr(),
            direction as i32,
            &mut shaped,
        )
    } != 0
    {
        return None;
    }
    let raw = if shaped.glyphs.is_null() {
        if shaped.count != 0 {
            unsafe { enzo_shaped_text_free(&mut shaped) };
            return None;
        }
        &[]
    } else {
        if shaped.count > isize::MAX as usize / mem::size_of::<EnzoShapedGlyph>() {
            unsafe { enzo_shaped_text_free(&mut shaped) };
            return None;
        }
        unsafe { slice::from_raw_parts(shaped.glyphs, shaped.count) }
    };
    let mut pen_x = 0_i64;
    let glyphs = raw
        .iter()
        .map(|glyph| {
            let positioned = PositionedGlyph {
                index: glyph.glyph_index,
                font_index: glyph.font_index,
                cluster: glyph.cluster,
                x: fixed_26_6_to_pixels(pen_x + i64::from(glyph.x_offset)),
                y: fixed_26_6_to_pixels(i64::from(glyph.y_offset)),
            };
            pen_x = pen_x.saturating_add(i64::from(glyph.x_advance));
            positioned
        })
        .collect();
    let direction = if shaped.paragraph_rtl != 0 {
        ParagraphDirection::RightToLeft
    } else {
        ParagraphDirection::LeftToRight
    };
    unsafe { enzo_shaped_text_free(&mut shaped) };
    Some(TextLayout {
        glyphs,
        width: fixed_26_6_to_pixels(pen_x).unsigned_abs(),
        direction,
    })
}

fn fixed_26_6_to_pixels(value: i64) -> i32 {
    if value >= 0 {
        ((value + 32) >> 6).min(i64::from(i32::MAX)) as i32
    } else {
        -(((-value + 32) >> 6).min(i64::from(i32::MAX)) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_boundaries_never_split_a_combining_sequence() {
        let text = "ب\u{064e}A";
        let layout = TextLayout {
            glyphs: vec![
                PositionedGlyph {
                    index: 1,
                    font_index: 1,
                    cluster: 0,
                    x: 0,
                    y: 0,
                },
                PositionedGlyph {
                    index: 2,
                    font_index: 1,
                    cluster: 0,
                    x: 4,
                    y: 0,
                },
                PositionedGlyph {
                    index: 3,
                    font_index: 0,
                    cluster: 2,
                    x: 8,
                    y: 0,
                },
            ],
            width: 12,
            direction: ParagraphDirection::RightToLeft,
        };

        assert_eq!(layout.cluster_boundaries(text), vec![0, 4, 5]);
    }
}
