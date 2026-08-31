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
