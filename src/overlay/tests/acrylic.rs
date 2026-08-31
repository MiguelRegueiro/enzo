use super::*;
use crate::overlay::{
    raster::{RoundedRect, fill_rounded_rect, rgb_offset},
    style::PANEL_COLOR,
};

#[test]
fn acrylic_blur_softens_pixels_inside_rounded_rect_only() {
    let width = 80;
    let height = 40;
    let mut frame = vec![0_u8; (width * height * 3) as usize];
    for y in 0..height {
        for x in width / 2..width {
            let offset = rgb_offset(width, x, y);
            frame[offset] = 240;
            frame[offset + 1] = 240;
            frame[offset + 2] = 240;
        }
    }
    let mut acrylic = AcrylicScratch::default();

    blur_rounded_rect(
        &mut frame,
        width,
        height,
        RoundedRect {
            x: 20.0,
            y: 20.0,
            width: 40.0,
            height: 12.0,
            radius: 4.0,
        },
        6,
        &mut acrylic,
    );

    let softened_offset = rgb_offset(width, 38, 26);
    assert!(frame[softened_offset] > 0);
    assert!(frame[softened_offset] < 240);

    let outside_offset = rgb_offset(width, 38, 5);
    assert_eq!(frame[outside_offset], 0);
    assert_eq!(frame[outside_offset + 1], 0);
    assert_eq!(frame[outside_offset + 2], 0);
}

#[test]
fn fused_acrylic_pass_matches_separate_blur_and_tint() {
    let width = 96;
    let height = 48;
    let source = (0_u32..width * height * 3)
        .map(|index| (index.wrapping_mul(37) % 251) as u8)
        .collect::<Vec<_>>();
    let rect = RoundedRect {
        x: 11.25,
        y: 7.5,
        width: 68.5,
        height: 29.25,
        radius: 6.0,
    };
    let mut expected = source.clone();
    let mut expected_scratch = AcrylicScratch::default();
    blur_rounded_rect(
        &mut expected,
        width,
        height,
        rect,
        ACRYLIC_BLUR_RADIUS,
        &mut expected_scratch,
    );
    fill_rounded_rect(&mut expected, width, height, rect, PANEL_COLOR, 202);

    let mut actual = source;
    let mut actual_scratch = AcrylicScratch::default();
    fill_acrylic_rounded_rect(
        &mut actual,
        width,
        height,
        rect,
        PANEL_COLOR,
        202,
        &mut actual_scratch,
    );

    assert_eq!(actual, expected);
}

#[test]
fn acrylic_workspace_reuses_capacity_without_changing_pixels() {
    let width = 128;
    let height = 72;
    let large_rect = RoundedRect {
        x: 4.0,
        y: 4.0,
        width: 116.0,
        height: 60.0,
        radius: 8.0,
    };
    let small_rect = RoundedRect {
        x: 20.0,
        y: 16.0,
        width: 52.0,
        height: 24.0,
        radius: 5.0,
    };
    let source = (0_u32..width * height * 3)
        .map(|index| (index.wrapping_mul(19) % 253) as u8)
        .collect::<Vec<_>>();
    let mut reused = AcrylicScratch::default();
    let mut warmup = source.clone();
    blur_rounded_rect(
        &mut warmup,
        width,
        height,
        large_rect,
        ACRYLIC_BLUR_RADIUS,
        &mut reused,
    );
    let capacities = (
        reused.source.capacity(),
        reused.horizontal.capacity(),
        reused.blurred.capacity(),
    );

    let mut actual = source.clone();
    blur_rounded_rect(
        &mut actual,
        width,
        height,
        small_rect,
        ACRYLIC_BLUR_RADIUS,
        &mut reused,
    );
    let mut expected = source;
    blur_rounded_rect(
        &mut expected,
        width,
        height,
        small_rect,
        ACRYLIC_BLUR_RADIUS,
        &mut AcrylicScratch::default(),
    );

    assert_eq!(actual, expected);
    assert_eq!(
        (
            reused.source.capacity(),
            reused.horizontal.capacity(),
            reused.blurred.capacity(),
        ),
        capacities
    );
}
