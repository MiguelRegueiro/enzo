//! Reusable backdrop-blur storage and acrylic panel compositing.

use super::raster::{
    RoundedRect, blend_pixel, fill_rounded_rect, rgb_offset, rounded_rect_coverage,
};

const ACRYLIC_BLUR_RADIUS: u32 = 12;

#[derive(Default)]
pub(super) struct AcrylicScratch {
    source: Vec<u8>,
    horizontal: Vec<u8>,
    blurred: Vec<u8>,
}
pub(super) fn fill_acrylic_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    color: [u8; 3],
    alpha: u8,
    scratch: &mut AcrylicScratch,
) {
    if !blur_rounded_rect_impl(
        frame,
        width,
        height,
        rect,
        ACRYLIC_BLUR_RADIUS,
        Some((color, alpha)),
        scratch,
    ) {
        fill_rounded_rect(frame, width, height, rect, color, alpha);
    }
}

#[cfg(test)]
fn blur_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    radius: u32,
    scratch: &mut AcrylicScratch,
) {
    blur_rounded_rect_impl(frame, width, height, rect, radius, None, scratch);
}

#[allow(clippy::too_many_arguments)]
fn blur_rounded_rect_impl(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    radius: u32,
    tint: Option<([u8; 3], u8)>,
    scratch: &mut AcrylicScratch,
) -> bool {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 || radius == 0 {
        return false;
    }

    let min_x = rect.x.floor().max(0.0) as u32;
    let max_x = (rect.x + rect.width).ceil().min(f64::from(width)) as u32;
    let min_y = rect.y.floor().max(0.0) as u32;
    let max_y = (rect.y + rect.height).ceil().min(f64::from(height)) as u32;
    if min_x >= max_x || min_y >= max_y {
        return false;
    }

    let sample_left = min_x.saturating_sub(radius);
    let sample_top = min_y.saturating_sub(radius);
    let sample_right = max_x.saturating_add(radius).min(width);
    let sample_bottom = max_y.saturating_add(radius).min(height);
    let sample_width = sample_right.saturating_sub(sample_left);
    let sample_height = sample_bottom.saturating_sub(sample_top);
    if sample_width == 0 || sample_height == 0 {
        return false;
    }

    let sample_len = (sample_width as usize)
        .saturating_mul(sample_height as usize)
        .saturating_mul(3);
    let AcrylicScratch {
        source,
        horizontal,
        blurred,
    } = scratch;
    source.resize(sample_len, 0);
    horizontal.resize(sample_len, 0);
    blurred.resize(sample_len, 0);
    for y in 0..sample_height {
        let source_start = rgb_offset(width, sample_left, sample_top + y);
        let source_end = source_start + sample_width as usize * 3;
        let target_start = (y * sample_width * 3) as usize;
        let target_end = target_start + sample_width as usize * 3;
        source[target_start..target_end].copy_from_slice(&frame[source_start..source_end]);
    }

    horizontal_box_blur_rgb(source, horizontal, sample_width, sample_height, radius);
    vertical_box_blur_rgb(horizontal, blurred, sample_width, sample_height, radius);

    for y in min_y..max_y {
        for x in min_x..max_x {
            let coverage = rounded_rect_coverage(f64::from(x) + 0.5, f64::from(y) + 0.5, rect);
            if coverage <= 0.0 {
                continue;
            }

            let source_offset = rgb_offset(sample_width, x - sample_left, y - sample_top);
            let target_offset = rgb_offset(width, x, y);
            let blurred_pixel = [
                blurred[source_offset],
                blurred[source_offset + 1],
                blurred[source_offset + 2],
            ];
            blend_pixel(
                frame,
                target_offset,
                blurred_pixel,
                (coverage * 255.0).round() as u8,
            );
            if let Some((color, alpha)) = tint {
                blend_pixel(
                    frame,
                    target_offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
    true
}

fn horizontal_box_blur_rgb(source: &[u8], target: &mut [u8], width: u32, height: u32, radius: u32) {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    if width == 0 || height == 0 {
        return;
    }

    for y in 0..height {
        let row_start = y * width * 3;
        let mut left = 0_usize;
        let mut right = 0_usize;
        let mut sum = [0_u32; 3];

        for x in 0..width {
            let wanted_left = x.saturating_sub(radius);
            let wanted_right = (x + radius).min(width - 1);

            while right <= wanted_right {
                let offset = row_start + right * 3;
                sum[0] += u32::from(source[offset]);
                sum[1] += u32::from(source[offset + 1]);
                sum[2] += u32::from(source[offset + 2]);
                right += 1;
            }

            while left < wanted_left {
                let offset = row_start + left * 3;
                sum[0] -= u32::from(source[offset]);
                sum[1] -= u32::from(source[offset + 1]);
                sum[2] -= u32::from(source[offset + 2]);
                left += 1;
            }

            let count = (right - left) as u32;
            let target_offset = row_start + x * 3;
            target[target_offset] = (sum[0] / count) as u8;
            target[target_offset + 1] = (sum[1] / count) as u8;
            target[target_offset + 2] = (sum[2] / count) as u8;
        }
    }
}

fn vertical_box_blur_rgb(source: &[u8], target: &mut [u8], width: u32, height: u32, radius: u32) {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    if width == 0 || height == 0 {
        return;
    }

    for x in 0..width {
        let mut top = 0_usize;
        let mut bottom = 0_usize;
        let mut sum = [0_u32; 3];

        for y in 0..height {
            let wanted_top = y.saturating_sub(radius);
            let wanted_bottom = (y + radius).min(height - 1);

            while bottom <= wanted_bottom {
                let offset = (bottom * width + x) * 3;
                sum[0] += u32::from(source[offset]);
                sum[1] += u32::from(source[offset + 1]);
                sum[2] += u32::from(source[offset + 2]);
                bottom += 1;
            }

            while top < wanted_top {
                let offset = (top * width + x) * 3;
                sum[0] -= u32::from(source[offset]);
                sum[1] -= u32::from(source[offset + 1]);
                sum[2] -= u32::from(source[offset + 2]);
                top += 1;
            }

            let count = (bottom - top) as u32;
            let target_offset = (y * width + x) * 3;
            target[target_offset] = (sum[0] / count) as u8;
            target[target_offset + 1] = (sum[1] / count) as u8;
            target[target_offset + 2] = (sum[2] / count) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
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
}
