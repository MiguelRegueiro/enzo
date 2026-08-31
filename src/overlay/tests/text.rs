use super::*;

#[test]
fn text_width_counts_spacing_between_glyphs_only() {
    assert_eq!(bitmap_text_width("12", 2), 22);
}

#[test]
fn bitmap_fallback_supports_media_info_text() {
    let text = "Source: E-AC-3 · Stereo · 48 kHz | Output: PCM S16 · Stereo · 48 kHz / H.264 · 513×289 · 24.0 fps · HDR (PQ)";

    assert!(text.chars().all(|character| glyph(character).is_some()));
}
