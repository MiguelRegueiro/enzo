use super::*;
use crate::terminal::ImageArea;

#[test]
fn seek_backward_saturates_at_start() {
    assert_eq!(
        seek_position(Duration::from_secs(3), -5, None),
        Duration::ZERO
    );
}

#[test]
fn seek_forward_clamps_to_duration() {
    assert_eq!(
        seek_position(Duration::from_secs(18), 5, Some(Duration::from_secs(20))),
        Duration::from_secs(20)
    );
}

#[test]
fn keyboard_seek_uses_fixed_five_second_steps() {
    assert_eq!(keyboard_seek_seconds(0), 0);
    assert_eq!(keyboard_seek_seconds(1), 5);
    assert_eq!(keyboard_seek_seconds(-1), -5);
    assert_eq!(keyboard_seek_seconds(4), 20);
    assert_eq!(keyboard_seek_seconds(-4), -20);
}

#[test]
fn keyboard_seek_step_calculation_saturates_safely() {
    assert_eq!(keyboard_seek_seconds(i32::MAX), i32::MAX);
    assert_eq!(keyboard_seek_seconds(i32::MIN), i32::MIN);
}

#[test]
fn exact_duration_seek_is_end_seek() {
    assert!(is_end_seek(
        Duration::from_secs(20),
        Some(Duration::from_secs(20))
    ));
}

#[test]
fn media_info_visibility_supports_temporary_and_pinned_modes() {
    let now = Instant::now();
    let mut info = MediaInfoOverlay::new(MediaInfo::new(String::new(), String::new(), Vec::new()));

    assert!(!info.visible(now));
    info.show(now);
    assert!(info.visible(now));
    assert!(!info.visible(now + MEDIA_INFO_VISIBLE_FOR));
    info.toggle();
    assert!(info.visible(now + MEDIA_INFO_VISIBLE_FOR));
}

#[test]
fn media_info_formats_compact_file_details() {
    assert_eq!(container_display_name("matroska,webm"), "Matroska");
    assert_eq!(container_display_name("mov,mp4,m4a"), "MP4 / MOV");
    assert_eq!(format_file_size(900), "900 B");
    assert_eq!(format_file_size(4 * 1024 * 1024 * 1024), "4.0 GiB");
}

#[test]
fn media_info_display_rate_visibility_matches_rendered_state() {
    let state = overlay_state(
        Duration::ZERO,
        None,
        None,
        false,
        None,
        None,
        false,
        None,
        false,
        Vec::new(),
        false,
        None,
        false,
        Vec::new(),
        "movie.mp4",
        Some(MediaInfoState {
            info: MediaInfo::new(String::new(), String::new(), Vec::new()),
            selected_audio: None,
            display_width: 640,
            display_height: 360,
            display_paused: false,
            display_fps: Some(24.0),
        }),
    );

    assert!(media_info_fps_visible(&state));
    let mut expired = state;
    expired.media_info.as_mut().expect("media info").display_fps = None;
    assert!(!media_info_fps_visible(&expired));

    assert_eq!(media_info_display_fps(false, Some(24.0)), Some(24.0));
    assert_eq!(media_info_display_fps(false, None), None);
    assert_eq!(media_info_display_fps(true, Some(24.0)), None);
}

#[test]
fn before_duration_seek_is_not_end_seek() {
    assert!(!is_end_seek(
        Duration::from_secs(19),
        Some(Duration::from_secs(20))
    ));
}

#[test]
fn overlay_is_visible_while_paused() {
    let now = Instant::now();

    assert!(overlay_visible(true, false, None, now));
}

#[test]
fn overlay_visibility_expires_when_playing() {
    let now = Instant::now();

    assert!(overlay_visible(
        false,
        false,
        Some(now + Duration::from_secs(1)),
        now
    ));
    assert!(!overlay_visible(
        false,
        false,
        Some(now - Duration::from_secs(1)),
        now
    ));
}

#[test]
fn overlay_is_visible_while_scrubbing() {
    let now = Instant::now();

    assert!(overlay_visible(false, true, None, now));
}

#[test]
fn overlay_state_uses_scrub_position() {
    let state = overlay_state(
        Duration::from_secs(10),
        Some(Duration::from_secs(30)),
        Some(Duration::from_secs(60)),
        false,
        None,
        None,
        false,
        None,
        false,
        Vec::new(),
        false,
        None,
        false,
        Vec::new(),
        "movie.mp4",
        None,
    );

    assert_eq!(state.position, Duration::from_secs(30));
    assert!(state.visible);
    assert_eq!(state.status_message, None);
    assert_eq!(state.media_title, Some("movie.mp4"));
}

#[test]
fn mouse_position_maps_terminal_cell_to_canvas_pixel() {
    let canvas = CanvasFrame {
        width: 1920,
        height: 1080,
        terminal_width: 1920,
        terminal_height: 1080,
        video_x: 0,
        video_y: 138,
        video_width: 1920,
        video_height: 804,
        overlay_scale_percent: 100,
        area: ImageArea {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        },
    };

    let point = mouse_canvas_position(40, 20, canvas).expect("point should be inside");

    assert_eq!(point.x, 972);
    assert_eq!(point.cell.left, point.x);
    assert_eq!(point.cell.right, point.x);
    assert_eq!(point.cell.top, 922);
    assert_eq!(point.cell.bottom, 922);
}

#[test]
fn mouse_position_maps_pixel_mouse_to_canvas_pixel() {
    let canvas = CanvasFrame {
        width: 1920,
        height: 1200,
        terminal_width: 2880,
        terminal_height: 1800,
        video_x: 0,
        video_y: 198,
        video_width: 1920,
        video_height: 804,
        overlay_scale_percent: 120,
        area: ImageArea {
            x: 0,
            y: 0,
            cols: 120,
            rows: 40,
        },
    };

    let point = mouse_canvas_position(1440, 1500, canvas).expect("point should be inside");

    assert_eq!(point.x, 960);
    assert_eq!(point.cell.left, 960);
    assert_eq!(point.cell.top, 1000);
    assert_eq!(point.cell.right, 960);
    assert_eq!(point.cell.bottom, 1000);
}

#[test]
fn copy_video_places_frame_inside_canvas() {
    let target = TargetFrame {
        width: 2,
        height: 2,
    };
    let canvas = CanvasFrame {
        width: 4,
        height: 4,
        terminal_width: 4,
        terminal_height: 4,
        video_x: 1,
        video_y: 1,
        video_width: 2,
        video_height: 2,
        overlay_scale_percent: 100,
        area: ImageArea {
            x: 0,
            y: 0,
            cols: 4,
            rows: 4,
        },
    };
    let frame = vec![
        1, 2, 3, 4, 5, 6, //
        7, 8, 9, 10, 11, 12,
    ];
    let mut canvas_frame = vec![0_u8; canvas.frame_len()];

    copy_video_into_canvas(&frame, target, &mut canvas_frame, canvas);

    let row_bytes = canvas.width as usize * 3;
    assert_eq!(
        &canvas_frame[row_bytes + 3..row_bytes + 9],
        &[1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        &canvas_frame[row_bytes * 2 + 3..row_bytes * 2 + 9],
        &[7, 8, 9, 10, 11, 12]
    );
    assert_eq!(&canvas_frame[..3], &[0, 0, 0]);
}

#[test]
fn copy_video_scales_frame_inside_canvas() {
    let target = TargetFrame {
        width: 2,
        height: 1,
    };
    let canvas = CanvasFrame {
        width: 4,
        height: 2,
        terminal_width: 4,
        terminal_height: 2,
        video_x: 0,
        video_y: 0,
        video_width: 4,
        video_height: 2,
        overlay_scale_percent: 100,
        area: ImageArea {
            x: 0,
            y: 0,
            cols: 4,
            rows: 2,
        },
    };
    let frame = vec![1, 2, 3, 7, 8, 9];
    let mut canvas_frame = vec![0_u8; canvas.frame_len()];

    copy_video_into_canvas(&frame, target, &mut canvas_frame, canvas);

    assert_eq!(&canvas_frame[..12], &[1, 2, 3, 1, 2, 3, 7, 8, 9, 7, 8, 9]);
    assert_eq!(&canvas_frame[12..24], &[1, 2, 3, 1, 2, 3, 7, 8, 9, 7, 8, 9]);
}

#[test]
fn progress_ratio_seek_uses_duration() {
    assert_eq!(
        seek_from_progress_ratio(0.25, Some(Duration::from_secs(80))),
        Some(Duration::from_secs(20))
    );
}
