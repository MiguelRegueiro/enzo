use super::*;

#[test]
fn default_font_can_draw_ascii_when_available() {
    let Some(path) = crate::font::FontSystem::discover()
        .resolve_all(crate::font::FontRole::Ui)
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
    let Some(path) = crate::font::FontSystem::discover()
        .resolve_all(crate::font::FontRole::Ui)
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
fn shaped_glyph_bitmaps_are_reused_across_subtitle_passes() {
    let Some(path) = crate::font::FontSystem::discover()
        .resolve_all(crate::font::FontRole::Ui)
        .next()
        .map(Path::to_path_buf)
    else {
        return;
    };
    let Some(mut font) = FontRenderer::open_path(&path, 20) else {
        return;
    };
    let Some(layout) = font.shape_text("Subtitle cache") else {
        return;
    };
    let mut frame = vec![0_u8; 240 * 48 * 3];

    for offset in -4..=4 {
        font.draw_text_layout(&mut frame, 240, 48, 8 + offset, 4, &layout, [255; 3], 255);
    }
    let first_pass_rasterizations = font.shaped_glyph_rasterizations;
    assert!(first_pass_rasterizations > 0);

    for offset in -4..=4 {
        font.draw_text_layout(&mut frame, 240, 48, 8 + offset, 4, &layout, [255; 3], 255);
    }

    assert_eq!(font.shaped_glyph_rasterizations, first_pass_rasterizations);
}

#[test]
fn arabic_text_is_shaped_and_mixed_runs_use_visual_order() {
    let system = crate::font::FontSystem::discover();
    let Some(path) = system
        .resolve_all_for_language(crate::font::FontRole::Subtitle, Some("ar"))
        .into_iter()
        .find(|path| FontRenderer::open_path(path, 24).is_some_and(|font| font.has_char('ب')))
    else {
        return;
    };
    let Some(mut font) = FontRenderer::open_path(&path, 24) else {
        return;
    };
    if !font.has_char('A') {
        for path in system.resolve_all(crate::font::FontRole::Ui) {
            if font.add_fallback_path(path) {
                break;
            }
        }
    }

    let isolated = font.shape_text("ب").expect("shape isolated Arabic");
    let joined = font.shape_text("بب").expect("shape joined Arabic");
    let isolated_twice = isolated
        .glyphs()
        .iter()
        .chain(isolated.glyphs())
        .map(|glyph| glyph.index)
        .collect::<Vec<_>>();
    let joined_ids = joined
        .glyphs()
        .iter()
        .map(|glyph| glyph.index)
        .collect::<Vec<_>>();
    assert!(joined_ids.iter().all(|&glyph| glyph != 0));
    assert_ne!(joined_ids, isolated_twice);

    let latin = font.shape_text("NETFLIX").expect("shape Latin run");
    let arabic = font.shape_text("مرحبا").expect("shape Arabic run");
    let space = font.shape_text(" ").expect("shape space").glyphs()[0];
    let mixed = font
        .shape_text("\u{202b}مرحبا NETFLIX\u{202c}")
        .expect("shape mixed bidi text");
    let visible_mixed = mixed
        .glyphs()
        .iter()
        .filter(|glyph| (glyph.font_index, glyph.index) != (space.font_index, space.index))
        .map(|glyph| (glyph.font_index, glyph.index))
        .collect::<Vec<_>>();
    let expected = latin
        .glyphs()
        .iter()
        .chain(arabic.glyphs())
        .map(|glyph| (glyph.font_index, glyph.index))
        .collect::<Vec<_>>();
    assert_eq!(visible_mixed, expected);
    assert!(mixed.width() > 0);
    assert!(
        mixed.glyphs().iter().all(|glyph| glyph.index != 0),
        "{mixed:?}"
    );
    let netflix_line = font
        .shape_text("\u{202b}\"مسلسلات أنيمي NETFLIX\"\u{202c}")
        .expect("shape Netflix Arabic subtitle");
    assert_eq!(netflix_line.direction(), ParagraphDirection::RightToLeft);
    assert!(netflix_line.glyphs().iter().all(|glyph| glyph.index != 0));

    let mut frame = vec![0_u8; 320 * 64 * 3];
    font.draw_text_layout(&mut frame, 320, 64, 4, 4, &mixed, [255; 3], 255);
    assert!(frame.iter().any(|&channel| channel != 0));
}

#[test]
fn fallback_keeps_an_arabic_base_and_mark_in_one_font_cluster() {
    let system = crate::font::FontSystem::discover();
    let Some(primary_path) = system
        .resolve_all(crate::font::FontRole::Ui)
        .find(|path| {
            FontRenderer::open_path(path, 24)
                .is_some_and(|font| font.has_char('A') && !font.has_char('ب'))
        })
        .map(Path::to_path_buf)
    else {
        return;
    };
    let Some(fallback_path) = system
        .resolve_all_for_language(crate::font::FontRole::Subtitle, Some("ar"))
        .into_iter()
        .find(|path| {
            FontRenderer::open_path(path, 24)
                .is_some_and(|font| font.has_char('ب') && font.has_char('\u{064e}'))
        })
    else {
        return;
    };
    let Some(mut font) = FontRenderer::open_path(&primary_path, 24) else {
        return;
    };

    assert!(font.add_fallback_path_for_text(&fallback_path, "ب\u{064e}A"));
    assert!(!font.add_fallback_path_for_text(&fallback_path, "ب\u{064e}A"));
    assert_eq!(font.fallback_count(), 1);
    let layout = font
        .shape_text("ب\u{064e}A")
        .expect("shape marked Arabic with Latin");
    let arabic_cluster = layout
        .glyphs()
        .iter()
        .filter(|glyph| glyph.cluster == 0)
        .collect::<Vec<_>>();

    assert!(!arabic_cluster.is_empty());
    assert!(arabic_cluster.iter().all(|glyph| glyph.font_index == 1));
    assert_eq!(layout.cluster_boundaries("ب\u{064e}A"), vec![0, 4, 5]);
}

#[test]
fn fallback_font_draws_a_glyph_missing_from_the_primary_face() {
    let system = crate::font::FontSystem::discover();
    let Some(primary_path) = system.resolve_all(crate::font::FontRole::Ui).next() else {
        return;
    };
    let Some(mut renderer) = FontRenderer::open_path(primary_path, 18) else {
        return;
    };
    if renderer.has_char('流') {
        return;
    }
    let Some(fallback_path) = system
        .resolve_all_for_language(crate::font::FontRole::Subtitle, Some("zh"))
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
    let Some(path) = crate::font::FontSystem::discover()
        .resolve_all(crate::font::FontRole::Ui)
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
