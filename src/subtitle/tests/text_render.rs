use super::*;
use crate::font::{FontRole, FontSystem};

#[test]
fn cached_text_overlay_matches_direct_subtitle_rendering() {
    let width = 320;
    let height = 180;
    let fallback_scale = fallback_text_scale(width, height);
    let line_height = 7 * fallback_scale;
    let line_gap = (line_height / 5).max(2);
    let lines = prepare_subtitle_lines(
        &[String::from("Cached subtitle")],
        width,
        fallback_scale,
        None,
    );
    let start_y = 120;
    let mut direct = (0..width * height * 3)
        .map(|index| (index.wrapping_mul(37) % 251) as u8)
        .collect::<Vec<_>>();
    let mut cached = direct.clone();
    let x = width.saturating_sub(lines[0].width) / 2;
    draw_prepared_subtitle_line(
        None,
        &mut direct,
        width,
        height,
        x,
        start_y,
        fallback_scale,
        &lines[0],
    );

    let overlay = build_text_overlay(
        None,
        width,
        height,
        start_y,
        line_height,
        line_gap,
        fallback_scale,
        &lines,
    )
    .expect("text overlay should be built");
    composite_text_overlay(&mut cached, &overlay);

    let max_difference = direct
        .iter()
        .zip(&cached)
        .map(|(&expected, &actual)| expected.abs_diff(actual))
        .max()
        .unwrap_or(0);
    assert!(
        max_difference <= 1,
        "maximum channel difference: {max_difference}"
    );
}

#[test]
fn wrapped_arabic_keeps_paragraph_direction_and_cluster_boundaries() {
    let system = FontSystem::discover();
    let Some(path) = system
        .resolve_all_for_language(FontRole::Subtitle, Some("ar"))
        .into_iter()
        .find(|path| {
            FontRenderer::open_path(path, 26).is_some_and(|font| font.has_char_for_test('م'))
        })
    else {
        return;
    };
    let Some(mut font) = FontRenderer::open_path(&path, 26) else {
        return;
    };
    let text = "مَرْحَبًامَرْحَبًامَرْحَبًا";
    let full_width = font.shape_text(text).expect("shape Arabic").width();
    let max_width = (full_width / 3).max(1);

    let lines = prepare_subtitle_lines(&[text.to_string()], max_width, 3, Some(&mut font));

    assert!(lines.len() > 1);
    assert!(lines.iter().all(|line| line.width <= max_width));
    assert!(lines.iter().all(|line| {
        line.layout
            .as_ref()
            .is_some_and(|layout| layout.direction() == ParagraphDirection::RightToLeft)
    }));
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        text
    );
}
