use std::time::Duration;

use super::*;
use crate::subtitle::{SubtitleRenderer, SubtitleTrack, track::SubtitleCue};

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
    let track = SubtitleTrack::from_cues(
        vec![SubtitleCue {
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
        Some("en".to_string()),
        String::from("English (en hdmv_pgs_subtitle)"),
    );
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
