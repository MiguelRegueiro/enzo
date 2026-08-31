use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void};

pub(super) const FT_LOAD_DEFAULT: c_int = 0;
pub(super) const FT_LOAD_RENDER: c_int = 4;

pub(super) type FtLibrary = *mut c_void;
pub(super) type FtFace = *mut FtFaceRec;
pub(super) type FtSize = *mut FtSizeRec;
pub(super) type FtGlyphSlot = *mut FtGlyphSlotRec;
pub(super) type FtPos = c_long;
pub(super) type FtFixed = c_long;

#[repr(C)]
pub(super) struct FtGeneric {
    pub(super) data: *mut c_void,
    pub(super) finalizer: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[repr(C)]
pub(super) struct FtBbox {
    pub(super) x_min: FtPos,
    pub(super) y_min: FtPos,
    pub(super) x_max: FtPos,
    pub(super) y_max: FtPos,
}

#[repr(C)]
pub(super) struct FtVector {
    pub(super) x: FtPos,
    pub(super) y: FtPos,
}

#[repr(C)]
pub(super) struct FtBitmap {
    pub(super) rows: c_uint,
    pub(super) width: c_uint,
    pub(super) pitch: c_int,
    pub(super) buffer: *mut c_uchar,
    pub(super) num_grays: c_ushort,
    pub(super) pixel_mode: c_uchar,
    pub(super) palette_mode: c_uchar,
    pub(super) palette: *mut c_void,
}

#[repr(C)]
pub(super) struct FtGlyphMetrics {
    pub(super) width: FtPos,
    pub(super) height: FtPos,
    pub(super) hori_bearing_x: FtPos,
    pub(super) hori_bearing_y: FtPos,
    pub(super) hori_advance: FtPos,
    pub(super) vert_bearing_x: FtPos,
    pub(super) vert_bearing_y: FtPos,
    pub(super) vert_advance: FtPos,
}

#[repr(C)]
pub(super) struct FtGlyphSlotRec {
    pub(super) library: FtLibrary,
    pub(super) face: FtFace,
    pub(super) next: FtGlyphSlot,
    pub(super) glyph_index: c_uint,
    pub(super) generic: FtGeneric,
    pub(super) metrics: FtGlyphMetrics,
    pub(super) linear_hori_advance: FtFixed,
    pub(super) linear_vert_advance: FtFixed,
    pub(super) advance: FtVector,
    pub(super) format: c_uint,
    pub(super) bitmap: FtBitmap,
    pub(super) bitmap_left: c_int,
    pub(super) bitmap_top: c_int,
}

#[repr(C)]
pub(super) struct FtSizeMetrics {
    pub(super) x_ppem: c_ushort,
    pub(super) y_ppem: c_ushort,
    pub(super) x_scale: FtFixed,
    pub(super) y_scale: FtFixed,
    pub(super) ascender: FtPos,
    pub(super) descender: FtPos,
    pub(super) height: FtPos,
    pub(super) max_advance: FtPos,
}

#[repr(C)]
pub(super) struct FtSizeRec {
    pub(super) face: FtFace,
    pub(super) generic: FtGeneric,
    pub(super) metrics: FtSizeMetrics,
    pub(super) internal: *mut c_void,
}

#[repr(C)]
pub(super) struct FtFaceRec {
    pub(super) num_faces: c_long,
    pub(super) face_index: c_long,
    pub(super) face_flags: c_long,
    pub(super) style_flags: c_long,
    pub(super) num_glyphs: c_long,
    pub(super) family_name: *mut c_char,
    pub(super) style_name: *mut c_char,
    pub(super) num_fixed_sizes: c_int,
    pub(super) available_sizes: *mut c_void,
    pub(super) num_charmaps: c_int,
    pub(super) charmaps: *mut c_void,
    pub(super) generic: FtGeneric,
    pub(super) bbox: FtBbox,
    pub(super) units_per_em: c_ushort,
    pub(super) ascender: c_short,
    pub(super) descender: c_short,
    pub(super) height: c_short,
    pub(super) max_advance_width: c_short,
    pub(super) max_advance_height: c_short,
    pub(super) underline_position: c_short,
    pub(super) underline_thickness: c_short,
    pub(super) glyph: FtGlyphSlot,
    pub(super) size: FtSize,
}

unsafe extern "C" {
    pub(super) fn FT_Init_FreeType(alibrary: *mut FtLibrary) -> c_int;
    pub(super) fn FT_Done_FreeType(library: FtLibrary) -> c_int;
    pub(super) fn FT_New_Face(
        library: FtLibrary,
        filepathname: *const c_char,
        face_index: c_long,
        aface: *mut FtFace,
    ) -> c_int;
    pub(super) fn FT_Done_Face(face: FtFace) -> c_int;
    pub(super) fn FT_Set_Pixel_Sizes(
        face: FtFace,
        pixel_width: c_uint,
        pixel_height: c_uint,
    ) -> c_int;
    pub(super) fn FT_Get_Char_Index(face: FtFace, charcode: c_ulong) -> c_uint;
    pub(super) fn FT_Load_Char(face: FtFace, char_code: c_ulong, load_flags: c_int) -> c_int;
    pub(super) fn FT_Load_Glyph(face: FtFace, glyph_index: c_uint, load_flags: c_int) -> c_int;
}
