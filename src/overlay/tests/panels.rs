use super::*;
use crate::overlay::{
    state::{MediaInfo, MediaInfoState},
    text::bitmap_text_width,
};

#[test]
fn display_info_describes_output_backend_size_and_measured_rate() {
    let playing = MediaInfoState {
        info: MediaInfo::new(String::new(), String::new(), Vec::new()),
        selected_audio: None,
        display_width: 1280,
        display_height: 720,
        display_paused: false,
        display_fps: Some(29.8),
    };

    assert_eq!(display_info_text(&playing), "Kitty · 1280×720 · 29.8 fps");

    let paused = MediaInfoState {
        display_paused: true,
        ..playing
    };
    assert_eq!(display_info_text(&paused), "Kitty · 1280×720 · paused");
}

#[test]
fn overlay_text_truncation_keeps_the_longest_fitting_prefix() {
    let mut font = None;
    let max_width = bitmap_text_width("ABCDE...", 1);

    assert_eq!(
        fit_overlay_text(&mut font, "ABCDEFGHIJ", 1, max_width),
        "ABCDE..."
    );
    assert_eq!(
        fit_overlay_text(&mut font, "éééééééééé", 1, max_width),
        "ééééé..."
    );
    assert_eq!(fit_overlay_text(&mut font, "ABCDE", 1, max_width), "ABCDE");
    assert_eq!(fit_overlay_text(&mut font, "ABCDE", 1, 0), "...");
}
