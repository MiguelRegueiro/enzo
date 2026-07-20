use crate::media::DecodedSubtitleBitmap;

use super::{SubtitleLayout, blend_pixel, rgb_offset, subtitle_bottom_margin};

pub(super) fn draw_bitmap_subtitle(
    frame: &mut [u8],
    layout: SubtitleLayout,
    bitmap: &DecodedSubtitleBitmap,
    bottom_reserve: u32,
) {
    if layout.video_width == 0
        || layout.video_height == 0
        || bitmap.width == 0
        || bitmap.height == 0
        || bitmap.canvas_width == 0
        || bitmap.canvas_height == 0
    {
        return;
    }
    let Some(rect) = bitmap_subtitle_rect(layout, bitmap, bottom_reserve) else {
        return;
    };

    let source_scale_x = f64::from(bitmap.width) / rect.width.max(1) as f64;
    let source_scale_y = f64::from(bitmap.height) / rect.height.max(1) as f64;
    for y in rect.draw_top..rect.draw_bottom {
        let source_y = (i64::from(y) - rect.top) as f64 * source_scale_y;
        for x in rect.draw_left..rect.draw_right {
            let source_x = (i64::from(x) - rect.left) as f64 * source_scale_x;
            let [red, green, blue, alpha] =
                sample_bitmap_subtitle_pixel(bitmap, source_x, source_y);
            if alpha == 0 {
                continue;
            }
            blend_pixel(
                frame,
                rgb_offset(layout.canvas_width, x, y),
                [red, green, blue],
                alpha,
            );
        }
    }
}

fn sample_bitmap_subtitle_pixel(bitmap: &DecodedSubtitleBitmap, x: f64, y: f64) -> [u8; 4] {
    let x = x.clamp(0.0, f64::from(bitmap.width.saturating_sub(1)));
    let y = y.clamp(0.0, f64::from(bitmap.height.saturating_sub(1)));
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = x0.saturating_add(1).min(bitmap.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(bitmap.height.saturating_sub(1));
    let tx = x - f64::from(x0);
    let ty = y - f64::from(y0);
    let top = mix_rgba(
        bitmap_subtitle_pixel(bitmap, x0, y0),
        bitmap_subtitle_pixel(bitmap, x1, y0),
        tx,
    );
    let bottom = mix_rgba(
        bitmap_subtitle_pixel(bitmap, x0, y1),
        bitmap_subtitle_pixel(bitmap, x1, y1),
        tx,
    );
    mix_rgba(top, bottom, ty)
}

fn bitmap_subtitle_pixel(bitmap: &DecodedSubtitleBitmap, x: u32, y: u32) -> [u8; 4] {
    let source_offset = (y * bitmap.width + x) as usize;
    let palette_offset = usize::from(bitmap.indices[source_offset]) * 4;
    [
        bitmap.palette_rgba[palette_offset],
        bitmap.palette_rgba[palette_offset + 1],
        bitmap.palette_rgba[palette_offset + 2],
        bitmap.palette_rgba[palette_offset + 3],
    ]
}

fn mix_rgba(left: [u8; 4], right: [u8; 4], amount: f64) -> [u8; 4] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|index| {
        (f64::from(left[index]) + (f64::from(right[index]) - f64::from(left[index])) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

struct BitmapSubtitleRect {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
    draw_left: u32,
    draw_top: u32,
    draw_right: u32,
    draw_bottom: u32,
}

fn bitmap_subtitle_rect(
    layout: SubtitleLayout,
    bitmap: &DecodedSubtitleBitmap,
    bottom_reserve: u32,
) -> Option<BitmapSubtitleRect> {
    let source_width = f64::from(bitmap.canvas_width);
    let source_height = f64::from(bitmap.canvas_height);
    let video_width = f64::from(layout.video_width);
    let video_height = f64::from(layout.video_height);
    let source_aspect = source_width / source_height;
    let video_aspect = video_width / video_height;

    let (source_x, source_y, source_view_width, source_view_height) =
        if video_aspect > source_aspect {
            let source_view_height = source_width / video_aspect;
            (
                0.0,
                ((source_height - source_view_height) / 2.0).max(0.0),
                source_width,
                source_view_height,
            )
        } else {
            let source_view_width = source_height * video_aspect;
            (
                ((source_width - source_view_width) / 2.0).max(0.0),
                0.0,
                source_view_width,
                source_height,
            )
        };

    let position_scale_x = video_width / source_view_width;
    let position_scale_y = video_height / source_view_height;
    let lower_subtitle = is_lower_subtitle(bitmap, source_height);
    let scale = bitmap_scale(
        lower_subtitle,
        position_scale_x,
        position_scale_y,
        video_height,
        source_height,
    );
    let width = (f64::from(bitmap.width) * scale).round().max(1.0) as i64;
    let height = (f64::from(bitmap.height) * scale).round().max(1.0) as i64;

    let (left, top, clip) = if lower_subtitle {
        lower_subtitle_rect_origin(layout, width, height, bottom_reserve)
    } else {
        authored_rect_origin(
            layout,
            bitmap,
            width,
            source_x,
            source_y,
            position_scale_x,
            position_scale_y,
        )
    };
    let right = left + width;
    let bottom = top + height;
    if width <= 0 || height <= 0 {
        return None;
    }
    let draw_left = left.max(clip.left);
    let draw_top = top.max(clip.top);
    let draw_right = right.min(clip.right);
    let draw_bottom = bottom.min(clip.bottom);
    if draw_left >= draw_right || draw_top >= draw_bottom {
        return None;
    }

    Some(BitmapSubtitleRect {
        left,
        top,
        width,
        height,
        draw_left: draw_left as u32,
        draw_top: draw_top as u32,
        draw_right: draw_right as u32,
        draw_bottom: draw_bottom as u32,
    })
}

fn is_lower_subtitle(bitmap: &DecodedSubtitleBitmap, source_height: f64) -> bool {
    // Dialogue PGS often keeps 1920x1080 Blu-ray coordinates even when the
    // encoded video is cropped (for example 1920x804). Treat bottom-half
    // bitmaps as normal dialogue so they align with Enzo text subtitles;
    // preserve authored viewport placement for upper signs and graphics.
    f64::from(bitmap.y) + f64::from(bitmap.height) / 2.0 >= source_height * 0.55
}

fn bitmap_scale(
    lower_subtitle: bool,
    position_scale_x: f64,
    position_scale_y: f64,
    video_height: f64,
    source_height: f64,
) -> f64 {
    if lower_subtitle {
        (video_height / source_height).clamp(0.5, 1.0)
    } else {
        position_scale_x.min(position_scale_y)
    }
}

#[derive(Clone, Copy)]
struct ClipRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

fn lower_subtitle_rect_origin(
    layout: SubtitleLayout,
    width: i64,
    height: i64,
    bottom_reserve: u32,
) -> (i64, i64, ClipRect) {
    let bottom_margin = subtitle_bottom_margin(layout.canvas_height)
        .max(bottom_reserve.saturating_add(8))
        .min(layout.canvas_height.saturating_sub(1));
    let left = (i64::from(layout.canvas_width) - width) / 2;
    let top = i64::from(layout.canvas_height.saturating_sub(bottom_margin)) - height;
    (
        left,
        top,
        ClipRect {
            left: 0,
            top: 0,
            right: i64::from(layout.canvas_width),
            bottom: i64::from(layout.canvas_height),
        },
    )
}

fn authored_rect_origin(
    layout: SubtitleLayout,
    bitmap: &DecodedSubtitleBitmap,
    width: i64,
    source_x: f64,
    source_y: f64,
    position_scale_x: f64,
    position_scale_y: f64,
) -> (i64, i64, ClipRect) {
    let center_x = f64::from(layout.video_x)
        + (f64::from(bitmap.x) + f64::from(bitmap.width) / 2.0 - source_x) * position_scale_x;
    let left = (center_x - width as f64 / 2.0).round() as i64;
    let top = (f64::from(layout.video_y) + (f64::from(bitmap.y) - source_y) * position_scale_y)
        .round() as i64;
    (
        left,
        top,
        ClipRect {
            left: i64::from(layout.video_x),
            top: i64::from(layout.video_y),
            right: i64::from(
                layout
                    .video_x
                    .saturating_add(layout.video_width)
                    .min(layout.canvas_width),
            ),
            bottom: i64::from(
                layout
                    .video_y
                    .saturating_add(layout.video_height)
                    .min(layout.canvas_height),
            ),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::subtitle::{SubtitleCue, SubtitleRenderer, SubtitleTrack};

    fn test_bitmap(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        canvas_width: u32,
        canvas_height: u32,
    ) -> DecodedSubtitleBitmap {
        DecodedSubtitleBitmap {
            x,
            y,
            width,
            height,
            canvas_width,
            canvas_height,
            indices: vec![1; (width * height) as usize],
            palette_rgba: Box::new([0_u8; 256 * 4]),
        }
    }

    #[test]
    fn bitmap_subtitles_preserve_bluray_canvas_position_on_cropped_video() {
        let bitmap = test_bitmap(760, 866, 400, 80, 1920, 1080);

        let rect = bitmap_subtitle_rect(
            SubtitleLayout {
                canvas_width: 1920,
                canvas_height: 804,
                video_x: 0,
                video_y: 0,
                video_width: 1920,
                video_height: 804,
            },
            &bitmap,
            0,
        )
        .unwrap();

        assert_eq!(rect.top, 698);
        assert_eq!(rect.height, 60);
    }

    #[test]
    fn bitmap_signs_scale_with_smaller_video_viewport() {
        let bitmap = test_bitmap(100, 100, 200, 80, 1920, 1080);

        let rect = bitmap_subtitle_rect(
            SubtitleLayout {
                canvas_width: 960,
                canvas_height: 540,
                video_x: 0,
                video_y: 0,
                video_width: 960,
                video_height: 540,
            },
            &bitmap,
            0,
        )
        .unwrap();

        assert_eq!(rect.left, 50);
        assert_eq!(rect.top, 50);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 40);
    }

    #[test]
    fn renderer_scales_bitmap_subtitle_into_video_viewport() {
        let mut palette_rgba = Box::new([0_u8; 256 * 4]);
        palette_rgba[4..8].copy_from_slice(&[12, 34, 56, 255]);
        let track = SubtitleTrack {
            cues: vec![SubtitleCue {
                start: Duration::from_secs(1),
                end: Duration::from_secs(2),
                lines: Vec::new(),
                bitmap: Some(DecodedSubtitleBitmap {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 2,
                    canvas_width: 4,
                    canvas_height: 4,
                    indices: vec![1; 4],
                    palette_rgba,
                }),
            }],
            language: Some("en".to_string()),
            label: String::from("English (en hdmv_pgs_subtitle)"),
        };
        let mut renderer = SubtitleRenderer::without_font();
        let mut frame = vec![0_u8; 8 * 8 * 3];

        renderer.render(
            &mut frame,
            SubtitleLayout {
                canvas_width: 8,
                canvas_height: 8,
                video_x: 2,
                video_y: 2,
                video_width: 4,
                video_height: 4,
            },
            &track,
            Duration::from_millis(1500),
            0,
        );

        assert_eq!(&frame[rgb_offset(8, 3, 3)..][..3], &[12, 34, 56]);
        assert_eq!(&frame[rgb_offset(8, 2, 2)..][..3], &[0, 0, 0]);
    }
}
