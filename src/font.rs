use std::{
    collections::HashMap,
    ffi::CString,
    os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void},
    path::Path,
    ptr,
};

const FT_LOAD_DEFAULT: c_int = 0;
const FT_LOAD_RENDER: c_int = 4;

fn is_bidi_format_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

type FtLibrary = *mut c_void;
type FtFace = *mut FtFaceRec;
type FtSize = *mut FtSizeRec;
type FtGlyphSlot = *mut FtGlyphSlotRec;
type FtPos = c_long;
type FtFixed = c_long;

#[repr(C)]
struct FtGeneric {
    data: *mut c_void,
    finalizer: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
struct FtBbox {
    x_min: FtPos,
    y_min: FtPos,
    x_max: FtPos,
    y_max: FtPos,
}

#[repr(C)]
struct FtVector {
    x: FtPos,
    y: FtPos,
}

#[repr(C)]
struct FtBitmap {
    rows: c_uint,
    width: c_uint,
    pitch: c_int,
    buffer: *mut c_uchar,
    num_grays: c_ushort,
    pixel_mode: c_uchar,
    palette_mode: c_uchar,
    palette: *mut c_void,
}

#[repr(C)]
struct FtGlyphMetrics {
    width: FtPos,
    height: FtPos,
    hori_bearing_x: FtPos,
    hori_bearing_y: FtPos,
    hori_advance: FtPos,
    vert_bearing_x: FtPos,
    vert_bearing_y: FtPos,
    vert_advance: FtPos,
}

#[repr(C)]
struct FtGlyphSlotRec {
    library: FtLibrary,
    face: FtFace,
    next: FtGlyphSlot,
    glyph_index: c_uint,
    generic: FtGeneric,
    metrics: FtGlyphMetrics,
    linear_hori_advance: FtFixed,
    linear_vert_advance: FtFixed,
    advance: FtVector,
    format: c_uint,
    bitmap: FtBitmap,
    bitmap_left: c_int,
    bitmap_top: c_int,
}

#[repr(C)]
struct FtSizeMetrics {
    x_ppem: c_ushort,
    y_ppem: c_ushort,
    x_scale: FtFixed,
    y_scale: FtFixed,
    ascender: FtPos,
    descender: FtPos,
    height: FtPos,
    max_advance: FtPos,
}

#[repr(C)]
struct FtSizeRec {
    face: FtFace,
    generic: FtGeneric,
    metrics: FtSizeMetrics,
    internal: *mut c_void,
}

#[repr(C)]
struct FtFaceRec {
    num_faces: c_long,
    face_index: c_long,
    face_flags: c_long,
    style_flags: c_long,
    num_glyphs: c_long,
    family_name: *mut c_char,
    style_name: *mut c_char,
    num_fixed_sizes: c_int,
    available_sizes: *mut c_void,
    num_charmaps: c_int,
    charmaps: *mut c_void,
    generic: FtGeneric,
    bbox: FtBbox,
    units_per_em: c_ushort,
    ascender: c_short,
    descender: c_short,
    height: c_short,
    max_advance_width: c_short,
    max_advance_height: c_short,
    underline_position: c_short,
    underline_thickness: c_short,
    glyph: FtGlyphSlot,
    size: FtSize,
}

unsafe extern "C" {
    fn FT_Init_FreeType(alibrary: *mut FtLibrary) -> c_int;
    fn FT_Done_FreeType(library: FtLibrary) -> c_int;
    fn FT_New_Face(
        library: FtLibrary,
        filepathname: *const c_char,
        face_index: c_long,
        aface: *mut FtFace,
    ) -> c_int;
    fn FT_Done_Face(face: FtFace) -> c_int;
    fn FT_Set_Pixel_Sizes(face: FtFace, pixel_width: c_uint, pixel_height: c_uint) -> c_int;
    fn FT_Get_Char_Index(face: FtFace, charcode: c_ulong) -> c_uint;
    fn FT_Load_Char(face: FtFace, char_code: c_ulong, load_flags: c_int) -> c_int;
}

pub(crate) struct FontRenderer {
    library: FtLibrary,
    face: FtFace,
    pixel_size: u32,
    ascii_glyphs: HashMap<char, CachedGlyph>,
    fallbacks: Vec<FontRenderer>,
}

struct CachedGlyph {
    advance: i32,
    rasterized: bool,
    bitmap: Option<CachedBitmap>,
}

struct CachedBitmap {
    left: i32,
    top: i32,
    width: u32,
    rows: u32,
    coverage: Vec<u8>,
}

impl FontRenderer {
    pub(crate) fn set_pixel_size(&mut self, pixel_size: u32) -> bool {
        let pixel_size = pixel_size.max(1);
        if self.pixel_size == pixel_size {
            return true;
        }

        let ok = unsafe { FT_Set_Pixel_Sizes(self.face, 0, pixel_size as c_uint) } == 0;
        if ok {
            self.pixel_size = pixel_size;
            self.ascii_glyphs.clear();
            self.fallbacks
                .retain_mut(|fallback| fallback.set_pixel_size(pixel_size));
        }
        ok
    }

    pub(crate) fn line_height(&self) -> u32 {
        self.size_metric(|metrics| to_pixels(metrics.height))
            .unwrap_or(self.pixel_size)
            .max(self.pixel_size)
    }

    pub(crate) fn text_width(&mut self, text: &str) -> u32 {
        let mut width = 0_i32;
        for ch in text.chars() {
            if is_bidi_format_control(ch) {
                continue;
            }
            let fallback = (!self.has_char(ch))
                .then(|| {
                    self.fallbacks
                        .iter_mut()
                        .find(|fallback| fallback.has_char(ch))
                })
                .flatten();
            let advance = if let Some(fallback) = fallback {
                fallback.char_advance(ch)
            } else if ch.is_ascii() {
                self.ascii_advance(ch)
            } else if self.load_char(ch, FT_LOAD_DEFAULT) {
                Some(self.current_advance())
            } else {
                None
            };
            if let Some(advance) = advance {
                width = width.saturating_add(advance);
            }
        }
        width.max(0) as u32
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_text(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        text: &str,
        color: [u8; 3],
        alpha: u8,
    ) {
        let baseline = y.saturating_add(self.ascent());
        let mut pen_x = x;

        for ch in text.chars() {
            if is_bidi_format_control(ch) {
                continue;
            }
            if !self.has_char(ch)
                && let Some(fallback) = self
                    .fallbacks
                    .iter_mut()
                    .find(|fallback| fallback.has_char(ch))
            {
                let mut encoded = [0; 4];
                let text = ch.encode_utf8(&mut encoded);
                let advance = fallback.text_width(text) as i32;
                fallback.draw_text(frame, width, height, pen_x, y, text, color, alpha);
                pen_x = pen_x.saturating_add(advance);
                continue;
            }
            if ch.is_ascii() {
                if !self.ensure_ascii_glyph(ch) {
                    continue;
                }
                let glyph = &self.ascii_glyphs[&ch];
                draw_cached_glyph(frame, width, height, pen_x, baseline, glyph, color, alpha);
                pen_x = pen_x.saturating_add(glyph.advance);
                continue;
            }
            if !self.load_char(ch, FT_LOAD_RENDER) {
                continue;
            }
            self.draw_current_glyph(frame, width, height, pen_x, baseline, color, alpha);
            pen_x = pen_x.saturating_add(self.current_advance());
        }
    }

    pub(crate) fn open_path(path: &Path, pixel_size: u32) -> Option<Self> {
        if !path.is_file() {
            return None;
        }

        let path = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
        let mut library = ptr::null_mut();
        if unsafe { FT_Init_FreeType(&mut library) } != 0 {
            return None;
        }

        let mut face = ptr::null_mut();
        if unsafe { FT_New_Face(library, path.as_ptr(), 0, &mut face) } != 0 {
            unsafe {
                FT_Done_FreeType(library);
            }
            return None;
        }

        let mut renderer = Self {
            library,
            face,
            pixel_size: 0,
            ascii_glyphs: HashMap::new(),
            fallbacks: Vec::new(),
        };
        if !renderer.set_pixel_size(pixel_size) {
            return None;
        }
        Some(renderer)
    }

    pub(crate) fn add_fallback_path(&mut self, path: &Path) -> bool {
        let Some(fallback) = Self::open_path(path, self.pixel_size) else {
            return false;
        };
        self.fallbacks.push(fallback);
        true
    }

    fn ascent(&self) -> i32 {
        self.size_metric(|metrics| to_pixels(metrics.ascender))
            .unwrap_or(self.pixel_size) as i32
    }

    fn size_metric(&self, read: impl FnOnce(&FtSizeMetrics) -> u32) -> Option<u32> {
        if self.face.is_null() {
            return None;
        }
        let size = unsafe { (*self.face).size };
        if size.is_null() {
            return None;
        }
        Some(read(unsafe { &(*size).metrics }))
    }

    fn load_char(&mut self, ch: char, flags: c_int) -> bool {
        (unsafe { FT_Load_Char(self.face, ch as c_ulong, flags) }) == 0
    }

    fn has_char(&self, ch: char) -> bool {
        !self.face.is_null() && unsafe { FT_Get_Char_Index(self.face, ch as c_ulong) } != 0
    }

    fn char_advance(&mut self, ch: char) -> Option<i32> {
        if ch.is_ascii() {
            self.ascii_advance(ch)
        } else if self.load_char(ch, FT_LOAD_DEFAULT) {
            Some(self.current_advance())
        } else {
            None
        }
    }

    fn ascii_advance(&mut self, ch: char) -> Option<i32> {
        if let Some(glyph) = self.ascii_glyphs.get(&ch) {
            return Some(glyph.advance);
        }
        if !self.load_char(ch, FT_LOAD_DEFAULT) {
            return None;
        }
        let advance = self.current_advance();
        self.ascii_glyphs.insert(
            ch,
            CachedGlyph {
                advance,
                rasterized: false,
                bitmap: None,
            },
        );
        Some(advance)
    }

    fn ensure_ascii_glyph(&mut self, ch: char) -> bool {
        if self
            .ascii_glyphs
            .get(&ch)
            .is_some_and(|glyph| glyph.rasterized)
        {
            return true;
        }
        if !self.load_char(ch, FT_LOAD_RENDER) {
            return false;
        }
        self.ascii_glyphs.insert(ch, self.cache_current_glyph());
        true
    }

    fn cache_current_glyph(&self) -> CachedGlyph {
        let slot = self.glyph_slot();
        if slot.is_null() {
            return CachedGlyph {
                advance: 0,
                rasterized: true,
                bitmap: None,
            };
        }

        let slot = unsafe { &*slot };
        CachedGlyph {
            advance: to_pixels(slot.advance.x) as i32,
            rasterized: true,
            bitmap: cache_bitmap(slot),
        }
    }

    fn current_advance(&self) -> i32 {
        let slot = self.glyph_slot();
        if slot.is_null() {
            return 0;
        }

        to_pixels(unsafe { (*slot).advance.x }) as i32
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_current_glyph(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        pen_x: i32,
        baseline: i32,
        color: [u8; 3],
        alpha: u8,
    ) {
        let slot = self.glyph_slot();
        if slot.is_null() {
            return;
        }

        let slot = unsafe { &*slot };
        let bitmap = &slot.bitmap;
        if bitmap.buffer.is_null() || bitmap.width == 0 || bitmap.rows == 0 {
            return;
        }

        let glyph_x = pen_x.saturating_add(slot.bitmap_left);
        let glyph_y = baseline.saturating_sub(slot.bitmap_top);
        let pitch = bitmap.pitch;
        if pitch == 0 {
            return;
        }

        for row in 0..bitmap.rows {
            for col in 0..bitmap.width {
                let Some(coverage) = bitmap_coverage(bitmap, row, col, pitch) else {
                    continue;
                };
                if coverage == 0 {
                    continue;
                }

                let px = glyph_x.saturating_add(col as i32);
                let py = glyph_y.saturating_add(row as i32);
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }

                let effective_alpha = ((u16::from(coverage) * u16::from(alpha) + 127) / 255) as u8;
                let offset = rgb_offset(width, px as u32, py as u32);
                blend_pixel(frame, offset, color, effective_alpha);
            }
        }
    }

    fn glyph_slot(&self) -> FtGlyphSlot {
        if self.face.is_null() {
            ptr::null_mut()
        } else {
            unsafe { (*self.face).glyph }
        }
    }
}

fn cache_bitmap(slot: &FtGlyphSlotRec) -> Option<CachedBitmap> {
    let bitmap = &slot.bitmap;
    if bitmap.buffer.is_null() || bitmap.width == 0 || bitmap.rows == 0 || bitmap.pitch == 0 {
        return None;
    }

    let mut coverage =
        Vec::with_capacity((bitmap.width as usize).checked_mul(bitmap.rows as usize)?);
    for row in 0..bitmap.rows {
        for col in 0..bitmap.width {
            coverage.push(bitmap_coverage(bitmap, row, col, bitmap.pitch)?);
        }
    }
    Some(CachedBitmap {
        left: slot.bitmap_left,
        top: slot.bitmap_top,
        width: bitmap.width,
        rows: bitmap.rows,
        coverage,
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_cached_glyph(
    frame: &mut [u8],
    width: u32,
    height: u32,
    pen_x: i32,
    baseline: i32,
    glyph: &CachedGlyph,
    color: [u8; 3],
    alpha: u8,
) {
    let Some(bitmap) = glyph.bitmap.as_ref() else {
        return;
    };
    let glyph_x = pen_x.saturating_add(bitmap.left);
    let glyph_y = baseline.saturating_sub(bitmap.top);
    for row in 0..bitmap.rows {
        for col in 0..bitmap.width {
            let coverage = bitmap.coverage[(row * bitmap.width + col) as usize];
            if coverage == 0 {
                continue;
            }

            let px = glyph_x.saturating_add(col as i32);
            let py = glyph_y.saturating_add(row as i32);
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                continue;
            }

            let effective_alpha = ((u16::from(coverage) * u16::from(alpha) + 127) / 255) as u8;
            let offset = rgb_offset(width, px as u32, py as u32);
            blend_pixel(frame, offset, color, effective_alpha);
        }
    }
}

impl Drop for FontRenderer {
    fn drop(&mut self) {
        unsafe {
            if !self.face.is_null() {
                FT_Done_Face(self.face);
            }
            if !self.library.is_null() {
                FT_Done_FreeType(self.library);
            }
        }
    }
}

fn bitmap_coverage(bitmap: &FtBitmap, row: c_uint, col: c_uint, pitch: c_int) -> Option<u8> {
    let row = row as isize;
    let col = col as isize;
    let pitch = pitch as isize;
    let rows = bitmap.rows as isize;
    let offset = if pitch > 0 {
        row.checked_mul(pitch)?.checked_add(col)?
    } else {
        rows.checked_sub(1)?
            .checked_sub(row)?
            .checked_mul(-pitch)?
            .checked_add(col)?
    };

    Some(unsafe { *bitmap.buffer.offset(offset) })
}

fn to_pixels(value: FtPos) -> u32 {
    ((value + 32) >> 6).max(0) as u32
}

fn blend_pixel(frame: &mut [u8], offset: usize, color: [u8; 3], alpha: u8) {
    let inverse = u16::from(255 - alpha);
    let alpha = u16::from(alpha);
    for channel in 0..3 {
        let source = u16::from(color[channel]) * alpha;
        let dest = u16::from(frame[offset + channel]) * inverse;
        frame[offset + channel] = ((source + dest + 127) / 255) as u8;
    }
}

fn rgb_offset(width: u32, x: u32, y: u32) -> usize {
    ((y * width + x) * 3) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_can_draw_ascii_when_available() {
        let Some(path) = crate::font_system::FontSystem::discover()
            .resolve_all(crate::font_system::FontRole::Ui)
            .next()
            .map(Path::to_path_buf)
        else {
            return;
        };
        let Some(mut font) = FontRenderer::open_path(&path, 20) else {
            return;
        };
        let mut frame = vec![0_u8; 160 * 48 * 3];

        assert!(font.text_width("1:23 / 4:56") > 0);
        font.draw_text(
            &mut frame,
            160,
            48,
            4,
            4,
            "1:23 / 4:56",
            [255, 255, 255],
            255,
        );

        assert!(frame.iter().any(|&value| value > 0));
    }

    #[test]
    fn bidi_format_controls_are_invisible() {
        let Some(path) = crate::font_system::FontSystem::discover()
            .resolve_all(crate::font_system::FontRole::Ui)
            .next()
            .map(Path::to_path_buf)
        else {
            return;
        };
        let Some(mut font) = FontRenderer::open_path(&path, 20) else {
            return;
        };

        assert_eq!(
            font.text_width("\u{200e}NETFLIX"),
            font.text_width("NETFLIX")
        );

        let mut with_mark = vec![0_u8; 160 * 48 * 3];
        let mut without_mark = vec![0_u8; with_mark.len()];
        font.draw_text(
            &mut with_mark,
            160,
            48,
            4,
            4,
            "\u{200e}NETFLIX",
            [255; 3],
            255,
        );
        font.draw_text(&mut without_mark, 160, 48, 4, 4, "NETFLIX", [255; 3], 255);

        assert_eq!(with_mark, without_mark);
    }

    #[test]
    fn fallback_font_draws_a_glyph_missing_from_the_primary_face() {
        let system = crate::font_system::FontSystem::discover();
        let Some(primary_path) = system.resolve_all(crate::font_system::FontRole::Ui).next() else {
            return;
        };
        let Some(mut renderer) = FontRenderer::open_path(primary_path, 18) else {
            return;
        };
        if renderer.has_char('流') {
            return;
        }
        let Some(fallback_path) = system
            .resolve_all_for_language(crate::font_system::FontRole::Subtitle, Some("zh"))
            .into_iter()
            .find(|path| FontRenderer::open_path(path, 18).is_some_and(|font| font.has_char('流')))
        else {
            return;
        };

        assert!(renderer.add_fallback_path(&fallback_path));
        let mut frame = vec![0_u8; 64 * 32 * 3];
        renderer.draw_text(&mut frame, 64, 32, 0, 0, "流", [255; 3], 255);

        assert!(frame.iter().any(|&channel| channel != 0));
    }

    #[test]
    fn cached_ascii_render_matches_direct_freetype_render() {
        let Some(path) = crate::font_system::FontSystem::discover()
            .resolve_all(crate::font_system::FontRole::Ui)
            .next()
            .map(Path::to_path_buf)
        else {
            return;
        };
        let Some(mut direct) = FontRenderer::open_path(&path, 18) else {
            return;
        };
        let Some(mut cached) = FontRenderer::open_path(&path, 18) else {
            return;
        };
        let text = "DISPLAY  Kitty · 513×289 · 24.0 fps";
        let mut expected = vec![0_u8; 420 * 48 * 3];
        let mut pen_x = 4_i32;
        let baseline = 4_i32.saturating_add(direct.ascent());
        for ch in text.chars() {
            if !direct.load_char(ch, FT_LOAD_RENDER) {
                continue;
            }
            direct.draw_current_glyph(
                &mut expected,
                420,
                48,
                pen_x,
                baseline,
                [255, 255, 255],
                244,
            );
            pen_x = pen_x.saturating_add(direct.current_advance());
        }

        let mut first = vec![0_u8; expected.len()];
        cached.draw_text(&mut first, 420, 48, 4, 4, text, [255, 255, 255], 244);
        let mut second = vec![0_u8; expected.len()];
        cached.draw_text(&mut second, 420, 48, 4, 4, text, [255, 255, 255], 244);

        assert_eq!(first, expected);
        assert_eq!(second, expected);
        assert_eq!(cached.text_width(text), direct.text_width(text));
    }
}
