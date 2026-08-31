use super::*;
use crate::overlay::style::TEXT_COLOR;

#[test]
fn rounded_rect_stroke_preserves_inner_pixels() {
    let width = 32;
    let height = 20;
    let mut frame = vec![20_u8; (width * height * 3) as usize];

    stroke_rounded_rect(
        &mut frame,
        width,
        height,
        RoundedRect {
            x: 4.0,
            y: 4.0,
            width: 24.0,
            height: 12.0,
            radius: 3.0,
        },
        2.0,
        TEXT_COLOR,
        255,
    );

    let border = rgb_offset(width, 4, 10);
    assert_eq!(&frame[border..border + 3], &TEXT_COLOR);

    let inner = rgb_offset(width, 16, 10);
    assert_eq!(&frame[inner..inner + 3], &[20, 20, 20]);
}
