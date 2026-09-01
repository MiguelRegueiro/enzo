use super::{
    centered_picker_offset_for_focus, close_help_on_outside_click, picker_owns_scroll,
    transient_ui_is_visible,
};

#[test]
fn an_open_track_picker_owns_mouse_wheel_input() {
    assert!(picker_owns_scroll(true, false));
    assert!(picker_owns_scroll(false, true));
    assert!(picker_owns_scroll(true, true));
    assert!(!picker_owns_scroll(false, false));
}

#[test]
fn outside_click_closes_help_and_resets_scroll() {
    let mut help_visible = true;
    let mut help_scroll_offset = 3;

    assert!(close_help_on_outside_click(
        &mut help_visible,
        &mut help_scroll_offset
    ));
    assert!(!help_visible);
    assert_eq!(help_scroll_offset, 0);
}

#[test]
fn scrub_preview_counts_as_transient_ui_state() {
    assert!(transient_ui_is_visible(
        false, false, false, false, None, None, true
    ));
    assert!(transient_ui_is_visible(
        false, true, false, false, None, None, false
    ));
}

#[test]
fn playlist_focus_opens_centered_when_space_allows() {
    assert_eq!(centered_picker_offset_for_focus(10, 30, 9), 6);
    assert_eq!(centered_picker_offset_for_focus(1, 30, 9), 0);
    assert_eq!(centered_picker_offset_for_focus(29, 30, 9), 21);
}
