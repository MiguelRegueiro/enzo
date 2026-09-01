use std::{
    sync::{Arc, Mutex, atomic::AtomicI32},
    time::Duration,
};

use super::*;

#[test]
fn stale_frame_is_not_published_after_seek_request() {
    let state = Arc::new(Mutex::new(LatestFrame::default()));
    let seek_generation = AtomicI32::new(2);
    let frame = vec![7, 8, 9];

    let buffer = store_latest_frame(&state, frame, Duration::from_secs(1), &seek_generation, 1);

    assert_eq!(buffer, vec![7, 8, 9]);
    let state = state.lock().expect("frame state should not be poisoned");
    assert!(state.frame.is_none());
    assert_eq!(state.serial, 0);
}

#[test]
fn seek_reset_keeps_the_reusable_frame_allocation() {
    let mut latest = LatestFrame::with_reusable_buffer(3).expect("frame buffer should allocate");
    latest.ready = true;
    let original = latest
        .frame
        .as_ref()
        .expect("reusable frame should exist")
        .as_ptr();
    let state = Arc::new(Mutex::new(latest));

    reset_frame_state(&state);

    let latest = state.lock().expect("frame state should not be poisoned");
    assert!(!latest.ready);
    assert_eq!(
        latest
            .frame
            .as_ref()
            .expect("seek should preserve the reusable frame")
            .as_ptr(),
        original
    );
}

#[test]
fn frame_guard_detects_native_output_overrun() {
    let mut frame = new_frame_buffer(3).expect("frame buffer should allocate");
    assert!(validate_frame_guard(&frame, 3).is_ok());

    frame[3] = 0;

    assert!(
        validate_frame_guard(&frame, 3)
            .expect_err("changed guard should be rejected")
            .to_string()
            .contains("past the RGB frame boundary")
    );
}
