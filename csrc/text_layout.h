#ifndef ENZO_TEXT_LAYOUT_H
#define ENZO_TEXT_LAYOUT_H

#include <stddef.h>
#include <stdint.h>

typedef struct EnzoShapedGlyph {
    uint32_t glyph_index;
    uint32_t font_index;
    uint32_t cluster;
    int32_t x_advance;
    int32_t x_offset;
    int32_t y_offset;
} EnzoShapedGlyph;

typedef struct EnzoShapedText {
    EnzoShapedGlyph *glyphs;
    size_t count;
    int32_t paragraph_rtl;
} EnzoShapedText;

enum {
    ENZO_TEXT_DIRECTION_AUTO = -1,
    ENZO_TEXT_DIRECTION_LTR = 0,
    ENZO_TEXT_DIRECTION_RTL = 1,
};

int enzo_shape_text(
    void *const *freetype_faces,
    size_t face_count,
    const char *utf8,
    int32_t paragraph_direction,
    EnzoShapedText *out
);
void enzo_shaped_text_free(EnzoShapedText *text);

#endif
