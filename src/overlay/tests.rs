//! Cross-module overlay composition and pixel-output regression tests.

use std::{sync::Arc, time::Duration};

use super::{
    acrylic::AcrylicScratch,
    layout::{OverlayMetrics, fallback_text_scale, text_size},
    raster::rgb_offset,
    rendering::render_overlay_rgb,
    state::{MediaInfo, MediaInfoState, OverlayState},
    timeline::{progress_pixels, time_column_width},
};

#[test]
fn paused_overlay_draws_play_button() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: true,
            visible: true,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: None,
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    let metrics = test_metrics(width, height);
    let offset = rgb_offset(
        width,
        metrics.inner_x + metrics.control_size / 2,
        metrics.control_y + metrics.control_size / 2,
    );
    assert!(frame[offset] > 180);
    assert!(frame[offset + 1] > 180);
    assert!(frame[offset + 2] > 180);
}

#[test]
fn playing_overlay_draws_pause_button() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: false,
            visible: true,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: None,
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    let metrics = test_metrics(width, height);
    let offset = rgb_offset(
        width,
        metrics.inner_x + metrics.control_size / 3,
        metrics.control_y + metrics.control_size / 2,
    );
    assert!(frame[offset] > 180);
    assert!(frame[offset + 1] > 180);
    assert!(frame[offset + 2] > 180);
}

#[test]
fn rendered_overlay_changes_bottom_pixels_only() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let before_top = frame[..(width * 20 * 3) as usize].to_vec();
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: true,
            visible: true,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: None,
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    assert_eq!(&frame[..before_top.len()], before_top.as_slice());
    assert!(
        frame
            .chunks_exact(3)
            .any(|pixel| pixel[0] > 200 && pixel[1] < 100 && pixel[2] < 100)
    );

    let metrics = test_metrics(width, height);
    let filled = progress_pixels(
        metrics.bar_width,
        Duration::from_secs(30),
        Some(Duration::from_secs(120)),
    );
    let handle_x = metrics.bar_x + filled;
    let handle_y = metrics.bar_y + metrics.bar_height / 2;
    let offset = rgb_offset(width, handle_x, handle_y);
    assert!(frame[offset] > 200);
    assert!(frame[offset + 1] < 120);
    assert!(frame[offset + 2] < 120);
}

#[test]
fn hidden_overlay_leaves_frame_unchanged() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let before = frame.clone();
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: false,
            visible: false,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: None,
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    assert_eq!(frame, before);
}

#[test]
fn status_message_can_render_without_playback_controls() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let before_top = frame[..(width * 40 * 3) as usize].to_vec();
    let before_bottom = frame[(width * 120 * 3) as usize..].to_vec();
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: false,
            visible: false,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: Some("MUTE ON"),
            media_title: None,
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    assert_ne!(&frame[..before_top.len()], before_top.as_slice());
    assert_eq!(
        &frame[(width * 120 * 3) as usize..],
        before_bottom.as_slice()
    );
}

#[test]
fn media_info_stacks_below_title_without_playback_controls() {
    let width = 640;
    let height = 360;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let before = frame.clone();
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: false,
            visible: false,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: Some(Arc::from("movie.mkv")),
            media_info: Some(MediaInfoState {
                info: MediaInfo::new(
                    "Matroska · 4.0 GiB".to_string(),
                    "HEVC · Main 10 · 3840×2160 · 59.94 fps".to_string(),
                    vec![
                        "Source: E-AC-3 · 5.1 · 48 kHz | Output: PCM S16 · Stereo · 48 kHz"
                            .to_string(),
                    ],
                ),
                selected_audio: Some(0),
                display_width: 1280,
                display_height: 720,
                display_paused: false,
                display_fps: Some(29.8),
            }),
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    assert_ne!(frame, before);
    assert_eq!(
        &frame[(width * 180 * 3) as usize..],
        &before[(width * 180 * 3) as usize..]
    );
}

#[test]
fn media_title_renders_with_playback_controls() {
    let width = 320;
    let height = 180;
    let mut frame = vec![20_u8; (width * height * 3) as usize];
    let before_top = frame[..(width * 40 * 3) as usize].to_vec();
    let mut scratch = String::new();
    let mut acrylic = AcrylicScratch::default();

    render_overlay_rgb(
        &mut frame,
        width,
        height,
        height as u16,
        100,
        OverlayState {
            position: Duration::from_secs(30),
            duration: Some(Duration::from_secs(120)),
            paused: false,
            visible: true,
            audio_available: false,
            selected_audio: None,
            audio_picker_open: false,
            audio_labels: Arc::default(),
            subtitles_available: false,
            selected_subtitle: None,
            subtitle_picker_open: false,
            subtitle_labels: Arc::default(),
            status_message: None,
            media_title: Some(Arc::from("movie.mkv")),
            media_info: None,
        },
        &mut scratch,
        &mut acrylic,
        None,
    );

    assert_ne!(&frame[..before_top.len()], before_top.as_slice());
}

fn test_metrics(width: u32, height: u32) -> OverlayMetrics {
    test_metrics_with_scale_and_controls(width, height, 100, false, false)
}

fn test_metrics_with_scale_and_controls(
    width: u32,
    height: u32,
    scale_percent: u32,
    audio_available: bool,
    subtitles_available: bool,
) -> OverlayMetrics {
    test_metrics_with_scale_controls_and_terminal_rows(
        width,
        height,
        height as u16,
        scale_percent,
        audio_available,
        subtitles_available,
    )
}

fn test_metrics_with_scale_controls_and_terminal_rows(
    width: u32,
    height: u32,
    terminal_rows: u16,
    scale_percent: u32,
    audio_available: bool,
    subtitles_available: bool,
) -> OverlayMetrics {
    let text_size = text_size(width, height, scale_percent);
    let fallback_text_scale = fallback_text_scale(width, height, scale_percent);
    let text_height = 7 * fallback_text_scale;
    let time_width = time_column_width(None, Some(Duration::from_secs(120)), fallback_text_scale);
    OverlayMetrics::new(
        width,
        height,
        text_size,
        fallback_text_scale,
        text_height,
        terminal_rows,
        time_width,
        audio_available,
        subtitles_available,
    )
}
