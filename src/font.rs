use std::{
    ffi::CString,
    os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void},
    path::Path,
    ptr,
};

const FT_LOAD_RENDER: c_int = 4;
const DEFAULT_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/TTF/OpenSans-Regular.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/TTF/Vera.ttf",
];

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
    fn FT_Load_Char(face: FtFace, char_code: c_ulong, load_flags: c_int) -> c_int;
}

pub(crate) struct FontRenderer {
    library: FtLibrary,
    face: FtFace,
    pixel_size: u32,
}

impl FontRenderer {
    pub(crate) fn open_default(pixel_size: u32) -> Option<Self> {
        DEFAULT_FONT_PATHS
            .iter()
            .find_map(|path| Self::open_path(path, pixel_size))
    }

    pub(crate) fn set_pixel_size(&mut self, pixel_size: u32) -> bool {
        let pixel_size = pixel_size.max(1);
        if self.pixel_size == pixel_size {
            return true;
        }

        let ok = unsafe { FT_Set_Pixel_Sizes(self.face, 0, pixel_size as c_uint) } == 0;
        if ok {
            self.pixel_size = pixel_size;
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
            if self.load_char(ch) {
                width = width.saturating_add(self.current_advance());
            }
        }
        width.max(0) as u32
    }

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
            if !self.load_char(ch) {
                continue;
            }
            self.draw_current_glyph(frame, width, height, pen_x, baseline, color, alpha);
            pen_x = pen_x.saturating_add(self.current_advance());
        }
    }

    fn open_path(path: &str, pixel_size: u32) -> Option<Self> {
        if !Path::new(path).is_file() {
            return None;
        }

        let path = CString::new(path).ok()?;
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
        };
        if !renderer.set_pixel_size(pixel_size) {
            return None;
        }
        Some(renderer)
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

    fn load_char(&mut self, ch: char) -> bool {
        (unsafe { FT_Load_Char(self.face, ch as c_ulong, FT_LOAD_RENDER) }) == 0
    }

    fn current_advance(&self) -> i32 {
        let slot = self.glyph_slot();
        if slot.is_null() {
            return 0;
        }

        to_pixels(unsafe { (*slot).advance.x }) as i32
    }

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
        let Some(mut font) = FontRenderer::open_default(20) else {
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
}
