#include "text_layout.h"

#include <fribidi.h>
#include <hb-ft.h>
#include <hb.h>
#include <limits.h>
#include <stdlib.h>
#include <string.h>

typedef struct BidiRun {
    FriBidiStrIndex start;
    FriBidiStrIndex len;
    FriBidiStrIndex visual_start;
    FriBidiLevel level;
} BidiRun;

static int compare_bidi_runs(const void *left, const void *right) {
    const BidiRun *left_run = left;
    const BidiRun *right_run = right;
    return (left_run->visual_start > right_run->visual_start)
        - (left_run->visual_start < right_run->visual_start);
}

static FriBidiStrIndex cluster_end(
    const FriBidiChar *text,
    FriBidiStrIndex len,
    FriBidiStrIndex start
);

static int append_shaped_run(
    hb_font_t *font,
    const FriBidiChar *text,
    FriBidiStrIndex start,
    FriBidiStrIndex len,
    int rtl,
    unsigned int font_index,
    EnzoShapedText *out
) {
    hb_buffer_t *buffer = hb_buffer_create();
    if (buffer == NULL || !hb_buffer_allocation_successful(buffer)) {
        hb_buffer_destroy(buffer);
        return -1;
    }
    FriBidiStrIndex run_end = start + len;
    for (FriBidiStrIndex cluster_start = start; cluster_start < run_end;) {
        FriBidiStrIndex end = cluster_end(text, run_end, cluster_start);
        for (FriBidiStrIndex index = cluster_start; index < end; index++) {
            hb_buffer_add(buffer, text[index], (unsigned int)cluster_start);
        }
        cluster_start = end;
    }
    hb_buffer_set_direction(buffer, rtl ? HB_DIRECTION_RTL : HB_DIRECTION_LTR);
    hb_buffer_set_flags(buffer, HB_BUFFER_FLAG_REMOVE_DEFAULT_IGNORABLES);
    hb_buffer_guess_segment_properties(buffer);
    hb_shape(font, buffer, NULL, 0);
    if (!hb_buffer_allocation_successful(buffer)) {
        hb_buffer_destroy(buffer);
        return -1;
    }

    unsigned int glyph_count = 0;
    hb_glyph_info_t *info = hb_buffer_get_glyph_infos(buffer, &glyph_count);
    hb_glyph_position_t *position = hb_buffer_get_glyph_positions(buffer, NULL);
    if (glyph_count > 0 && (info == NULL || position == NULL)) {
        hb_buffer_destroy(buffer);
        return -1;
    }
    if (glyph_count > SIZE_MAX / sizeof(*out->glyphs) - out->count) {
        hb_buffer_destroy(buffer);
        return -1;
    }
    size_t new_count = out->count + glyph_count;
    EnzoShapedGlyph *glyphs = realloc(out->glyphs, new_count * sizeof(*glyphs));
    if (new_count > 0 && glyphs == NULL) {
        hb_buffer_destroy(buffer);
        return -1;
    }
    out->glyphs = glyphs;
    for (unsigned int index = 0; index < glyph_count; index++) {
        FriBidiStrIndex cluster = start;
        while (cluster < run_end) {
            FriBidiStrIndex end = cluster_end(text, run_end, cluster);
            if (info[index].cluster < (unsigned int)end) {
                break;
            }
            cluster = end;
        }
        out->glyphs[out->count + index] = (EnzoShapedGlyph) {
            .glyph_index = info[index].codepoint,
            .font_index = font_index,
            .cluster = (unsigned int)cluster,
            .x_advance = position[index].x_advance,
            .x_offset = position[index].x_offset,
            .y_offset = position[index].y_offset,
        };
    }
    out->count = new_count;
    hb_buffer_destroy(buffer);
    return 0;
}

static int is_default_ignorable(FriBidiChar character) {
    return character == 0x00AD || character == 0x034F || character == 0x061C
        || (character >= 0x115F && character <= 0x1160)
        || (character >= 0x17B4 && character <= 0x17B5)
        || (character >= 0x180B && character <= 0x180F)
        || (character >= 0x200B && character <= 0x200F)
        || (character >= 0x202A && character <= 0x202E)
        || (character >= 0x2060 && character <= 0x206F)
        || character == 0x3164
        || (character >= 0xFE00 && character <= 0xFE0F)
        || character == 0xFEFF || character == 0xFFA0
        || (character >= 0x1BCA0 && character <= 0x1BCA3)
        || (character >= 0x1D173 && character <= 0x1D17A)
        || (character >= 0xE0000 && character <= 0xE0FFF);
}

static int is_cluster_mark(FriBidiChar character) {
    hb_unicode_general_category_t category = hb_unicode_general_category(
        hb_unicode_funcs_get_default(),
        character
    );
    return category == HB_UNICODE_GENERAL_CATEGORY_NON_SPACING_MARK
        || category == HB_UNICODE_GENERAL_CATEGORY_SPACING_MARK
        || category == HB_UNICODE_GENERAL_CATEGORY_ENCLOSING_MARK
        || fribidi_get_bidi_type(character) == FRIBIDI_TYPE_NSM
        || (character >= 0x1F3FB && character <= 0x1F3FF);
}

static FriBidiStrIndex cluster_end(
    const FriBidiChar *text,
    FriBidiStrIndex len,
    FriBidiStrIndex start
) {
    FriBidiStrIndex end = start + 1;
    int regional_indicators = text[start] >= 0x1F1E6 && text[start] <= 0x1F1FF;
    while (end < len) {
        FriBidiChar character = text[end];
        if (is_cluster_mark(character) || is_default_ignorable(character)) {
            int joins_next = hb_unicode_combining_class(
                hb_unicode_funcs_get_default(),
                character
            ) == HB_UNICODE_COMBINING_CLASS_VIRAMA;
            end++;
            if ((character == 0x200D || joins_next) && end < len) {
                end++;
            }
            continue;
        }
        if (regional_indicators == 1 && character >= 0x1F1E6 && character <= 0x1F1FF) {
            end++;
        }
        break;
    }
    return end;
}

static unsigned int face_for_cluster(
    void *const *faces,
    size_t face_count,
    const FriBidiChar *text,
    FriBidiStrIndex start,
    FriBidiStrIndex end
) {
    unsigned int best_face = 0;
    FriBidiStrIndex best_coverage = -1;
    for (size_t face_index = 0; face_index < face_count; face_index++) {
        FriBidiStrIndex required = 0;
        FriBidiStrIndex coverage = 0;
        for (FriBidiStrIndex index = start; index < end; index++) {
            if (is_default_ignorable(text[index])) {
                continue;
            }
            required++;
            coverage += FT_Get_Char_Index((FT_Face)faces[face_index], text[index]) != 0;
        }
        if (coverage == required) {
            return (unsigned int)face_index;
        }
        if (coverage > best_coverage) {
            best_face = (unsigned int)face_index;
            best_coverage = coverage;
        }
    }
    return best_face;
}

static void assign_cluster_fonts(
    void *const *faces,
    size_t face_count,
    const FriBidiChar *text,
    FriBidiStrIndex len,
    unsigned int *font_indices
) {
    for (FriBidiStrIndex start = 0; start < len;) {
        FriBidiStrIndex end = cluster_end(text, len, start);
        unsigned int font_index = face_for_cluster(
            faces,
            face_count,
            text,
            start,
            end
        );
        for (FriBidiStrIndex index = start; index < end; index++) {
            font_indices[index] = font_index;
        }
        start = end;
    }
}

static int append_font_runs(
    void *const *faces,
    const FriBidiChar *text,
    FriBidiStrIndex start,
    FriBidiStrIndex len,
    int rtl,
    const unsigned int *font_indices,
    EnzoShapedText *out
) {
    FriBidiStrIndex cursor = rtl ? start + len : start;
    FriBidiStrIndex limit = rtl ? start : start + len;
    while (cursor != limit) {
        FriBidiStrIndex character = rtl ? cursor - 1 : cursor;
        unsigned int font_index = font_indices[character];
        FriBidiStrIndex run_start = character;
        FriBidiStrIndex run_end = character + 1;
        if (rtl) {
            while (run_start > limit && font_indices[run_start - 1] == font_index) {
                run_start--;
            }
            cursor = run_start;
        } else {
            while (run_end < limit && font_indices[run_end] == font_index) {
                run_end++;
            }
            cursor = run_end;
        }

        hb_font_t *font = hb_ft_font_create_referenced((FT_Face)faces[font_index]);
        if (font == NULL) {
            return -1;
        }
        int status = append_shaped_run(
            font,
            text,
            run_start,
            run_end - run_start,
            rtl,
            font_index,
            out
        );
        hb_font_destroy(font);
        if (status != 0) {
            return -1;
        }
    }
    return 0;
}

int enzo_shape_text(
    void *const *freetype_faces,
    size_t face_count,
    const char *utf8,
    int32_t paragraph_direction,
    EnzoShapedText *out
) {
    if (freetype_faces == NULL || face_count == 0 || freetype_faces[0] == NULL
        || face_count > UINT_MAX || utf8 == NULL || out == NULL
        || paragraph_direction < ENZO_TEXT_DIRECTION_AUTO
        || paragraph_direction > ENZO_TEXT_DIRECTION_RTL) {
        return -1;
    }
    *out = (EnzoShapedText) {0};
    size_t utf8_len = strlen(utf8);
    if (utf8_len == 0) {
        return 0;
    }
    if (utf8_len > INT_MAX
        || utf8_len > SIZE_MAX / sizeof(FriBidiChar)
        || utf8_len > SIZE_MAX / sizeof(FriBidiCharType)
        || utf8_len > SIZE_MAX / sizeof(FriBidiBracketType)
        || utf8_len > SIZE_MAX / sizeof(FriBidiLevel)
        || utf8_len > SIZE_MAX / sizeof(FriBidiStrIndex)
        || utf8_len > SIZE_MAX / sizeof(unsigned int)
        || utf8_len > SIZE_MAX / sizeof(BidiRun)) {
        return -1;
    }

    FriBidiChar *logical = malloc(utf8_len * sizeof(*logical));
    FriBidiCharType *types = malloc(utf8_len * sizeof(*types));
    FriBidiBracketType *brackets = malloc(utf8_len * sizeof(*brackets));
    FriBidiLevel *levels = malloc(utf8_len * sizeof(*levels));
    FriBidiStrIndex *visual_to_logical = malloc(utf8_len * sizeof(*visual_to_logical));
    FriBidiStrIndex *logical_to_visual = malloc(utf8_len * sizeof(*logical_to_visual));
    unsigned int *font_indices = malloc(utf8_len * sizeof(*font_indices));
    BidiRun *runs = malloc(utf8_len * sizeof(*runs));
    if (logical == NULL || types == NULL || brackets == NULL || levels == NULL
        || visual_to_logical == NULL || logical_to_visual == NULL
        || font_indices == NULL || runs == NULL) {
        free(logical);
        free(types);
        free(brackets);
        free(levels);
        free(visual_to_logical);
        free(logical_to_visual);
        free(font_indices);
        free(runs);
        return -1;
    }

    FriBidiStrIndex len = fribidi_charset_to_unicode(
        FRIBIDI_CHAR_SET_UTF8,
        utf8,
        (FriBidiStrIndex)utf8_len,
        logical
    );
    fribidi_get_bidi_types(logical, len, types);
    fribidi_get_bracket_types(logical, len, types, brackets);
    FriBidiParType base_direction = paragraph_direction == ENZO_TEXT_DIRECTION_AUTO
        ? FRIBIDI_PAR_ON
        : (paragraph_direction == ENZO_TEXT_DIRECTION_LTR
            ? FRIBIDI_PAR_LTR
            : FRIBIDI_PAR_RTL);
    int ok = fribidi_get_par_embedding_levels_ex(
        types,
        brackets,
        len,
        &base_direction,
        levels
    ) != 0;
    out->paragraph_rtl = FRIBIDI_IS_RTL(base_direction);
    if (ok) {
        assign_cluster_fonts(
            freetype_faces,
            face_count,
            logical,
            len,
            font_indices
        );
    }
    for (FriBidiStrIndex index = 0; index < len; index++) {
        visual_to_logical[index] = index;
    }
    if (ok) {
        ok = fribidi_reorder_line(
            FRIBIDI_FLAGS_DEFAULT,
            types,
            len,
            0,
            base_direction,
            levels,
            NULL,
            visual_to_logical
        ) != 0;
    }

    for (FriBidiStrIndex visual = 0; ok && visual < len; visual++) {
        logical_to_visual[visual_to_logical[visual]] = visual;
    }
    size_t run_count = 0;
    for (FriBidiStrIndex start = 0; ok && start < len;) {
        FriBidiStrIndex end = start + 1;
        while (end < len && levels[end] == levels[start]) {
            end++;
        }
        FriBidiStrIndex visual_start = logical_to_visual[start];
        for (FriBidiStrIndex index = start + 1; index < end; index++) {
            if (logical_to_visual[index] < visual_start) {
                visual_start = logical_to_visual[index];
            }
        }
        runs[run_count++] = (BidiRun) {
            .start = start,
            .len = end - start,
            .visual_start = visual_start,
            .level = levels[start],
        };
        start = end;
    }
    qsort(runs, run_count, sizeof(*runs), compare_bidi_runs);

    for (size_t index = 0; ok && index < run_count; index++) {
        BidiRun run = runs[index];
        ok = append_font_runs(
            freetype_faces,
            logical,
            run.start,
            run.len,
            (run.level & 1) != 0,
            font_indices,
            out
        ) == 0;
    }

    free(logical);
    free(types);
    free(brackets);
    free(levels);
    free(visual_to_logical);
    free(logical_to_visual);
    free(font_indices);
    free(runs);
    if (!ok) {
        enzo_shaped_text_free(out);
        return -1;
    }
    return 0;
}

void enzo_shaped_text_free(EnzoShapedText *text) {
    if (text == NULL) {
        return;
    }
    free(text->glyphs);
    *text = (EnzoShapedText) {0};
}
