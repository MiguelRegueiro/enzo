use super::*;
use std::time::Duration;

#[test]
fn timestamp_omits_hours_when_short() {
    assert_eq!(format_timestamp(Duration::from_secs(65)), "1:05");
}

#[test]
fn timestamp_includes_hours_when_needed() {
    assert_eq!(format_timestamp(Duration::from_secs(3661)), "1:01:01");
}

#[test]
fn time_column_uses_hour_position_when_duration_has_hours() {
    let duration = Duration::from_secs(2 * 3600 + 6 * 60 + 44);

    assert_eq!(
        format_position_timestamp(Duration::ZERO, Some(duration)),
        "0:00:00"
    );
    assert_eq!(
        format_position_timestamp(Duration::from_secs(3600 + 37 * 60 + 38), Some(duration)),
        "1:37:38"
    );
    assert_eq!(time_column_template(Some(duration)), "0:00:00 / 2:06:44");
}

#[test]
fn time_column_keeps_minute_position_for_short_duration() {
    let duration = Duration::from_secs(6 * 60 + 44);

    assert_eq!(
        format_position_timestamp(Duration::ZERO, Some(duration)),
        "0:00"
    );
    assert_eq!(time_column_template(Some(duration)), "0:00 / 6:44");
}

#[test]
fn progress_pixels_clamps_to_width() {
    assert_eq!(
        progress_pixels(12, Duration::from_secs(30), Some(Duration::from_secs(10))),
        12
    );
}
