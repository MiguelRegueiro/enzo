use super::{DISABLE_MOUSE_TRACKING, ENABLE_MOUSE_TRACKING};

#[test]
fn pointer_tracking_keeps_movement_clicks_and_drags_in_cell_mode() {
    for mode in [
        b"\x1b[?1000h",
        b"\x1b[?1002h",
        b"\x1b[?1003h",
        b"\x1b[?1006h",
    ] {
        assert!(
            ENABLE_MOUSE_TRACKING
                .windows(mode.len())
                .any(|enabled| enabled == mode)
        );
    }
    for mode in [b"\x1b[?1015l", b"\x1b[?1016l"] {
        assert!(
            ENABLE_MOUSE_TRACKING
                .windows(mode.len())
                .any(|disabled| disabled == mode)
        );
    }
    for mode in [b"\x1b[?1015h", b"\x1b[?1016h"] {
        assert!(
            !ENABLE_MOUSE_TRACKING
                .windows(mode.len())
                .any(|enabled| enabled == mode)
        );
    }
}

#[test]
fn teardown_disables_every_mouse_mode_enzo_may_encounter() {
    for mode in [
        b"\x1b[?1000l",
        b"\x1b[?1002l",
        b"\x1b[?1003l",
        b"\x1b[?1006l",
        b"\x1b[?1015l",
        b"\x1b[?1016l",
    ] {
        assert!(
            DISABLE_MOUSE_TRACKING
                .windows(mode.len())
                .any(|disabled| disabled == mode)
        );
    }
}
