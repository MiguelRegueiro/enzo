use std::ffi::OsStr;

use super::*;

#[test]
fn passthrough_targets_the_current_tmux_pane_when_available() {
    assert_eq!(
        allow_passthrough_args(Some(OsStr::new("%7"))),
        vec![
            "set-option",
            "-p",
            "-q",
            "-t",
            "%7",
            "allow-passthrough",
            "on"
        ]
    );
}

#[test]
fn passthrough_falls_back_to_tmux_current_pane_resolution() {
    assert_eq!(
        allow_passthrough_args(None),
        vec!["set-option", "-p", "-q", "allow-passthrough", "on"]
    );
    assert_eq!(
        allow_passthrough_args(Some(OsStr::new(""))),
        vec!["set-option", "-p", "-q", "allow-passthrough", "on"]
    );
}
