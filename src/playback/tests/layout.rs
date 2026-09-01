use super::*;

#[test]
fn target_caps_large_sources_at_1080p() {
    let target = target_for_bounds(3840, 2160, 3840, 2160);
    assert_eq!(target.width, 1920);
    assert_eq!(target.height, 1080);
}

#[test]
fn target_upscales_small_sources_to_the_display_bounds() {
    let target = target_for_bounds(1280, 720, 3840, 2160);
    assert_eq!(target.width, 1920);
    assert_eq!(target.height, 1080);
}

#[test]
fn target_preserves_aspect_inside_1080p_cap() {
    let target = target_for_bounds(2560, 1080, 3840, 2160);
    assert_eq!(target.width, 1920);
    assert_eq!(target.height, 810);
}

#[test]
fn canvas_uses_terminal_letterbox_space() {
    let canvas = canvas_for_terminal(1280, 536, 80, 24, 1920, 1080);
    assert_eq!(
        canvas,
        CanvasFrame {
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
        }
    );
}

#[test]
fn canvas_caps_high_density_terminals() {
    let canvas = canvas_for_terminal(1280, 536, 120, 40, 2880, 1800);
    assert_eq!(
        canvas,
        CanvasFrame {
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
        }
    );
}
