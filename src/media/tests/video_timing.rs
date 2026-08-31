use std::{
    sync::{Arc, Mutex, atomic::AtomicI64},
    time::{Duration, Instant},
};

use super::*;
use crate::media::ffi_support::duration_micros_i64;

#[test]
fn display_rate_measures_recent_frame_delivery() {
    let start = Instant::now();
    let mut rate = DisplayRate::default();
    rate.record(start);
    rate.record(start + Duration::from_millis(40));

    assert_eq!(
        rate.measured_at(start + Duration::from_millis(40)),
        Some(25.0)
    );
    assert_eq!(
        rate.measured_at(start + DISPLAY_RATE_WINDOW + Duration::from_secs(1)),
        None
    );
}

#[test]
fn stale_frame_drop_threshold_trails_audio_clock() {
    let master_clock = Mutex::new(Some(Arc::new(AtomicI64::new(duration_micros_i64(
        Duration::from_millis(500),
    )))));

    assert_eq!(
        stale_frame_drop_before(&master_clock),
        Some(Duration::from_millis(425))
    );

    let master_clock = Mutex::new(Some(Arc::new(AtomicI64::new(duration_micros_i64(
        Duration::from_millis(50),
    )))));

    assert_eq!(stale_frame_drop_before(&master_clock), None);
}

#[test]
fn clock_drop_policy_bounds_display_starvation() {
    let now = Instant::now();

    assert!(!should_publish_late_frame(
        MAX_CONSECUTIVE_CLOCK_DROPS - 1,
        now,
        now + CLOCK_DROP_STARVATION_LIMIT - Duration::from_millis(1),
    ));
    assert!(should_publish_late_frame(
        MAX_CONSECUTIVE_CLOCK_DROPS,
        now,
        now,
    ));
    assert!(should_publish_late_frame(
        0,
        now,
        now + CLOCK_DROP_STARVATION_LIMIT,
    ));
}
