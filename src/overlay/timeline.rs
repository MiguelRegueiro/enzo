//! Playback timeline formatting and progress calculations.

use std::time::Duration;

use crate::font::FontRenderer;

use super::text::bitmap_text_width;

pub(super) fn time_column_width(
    font: Option<&mut FontRenderer>,
    duration: Option<Duration>,
    fallback_scale: u32,
) -> u32 {
    let text = time_column_template(duration);
    font.map(|font| font.text_width(&text))
        .unwrap_or_else(|| bitmap_text_width(&text, fallback_scale))
}

fn time_column_template(duration: Option<Duration>) -> String {
    if let Some(duration) = duration {
        let position = format_position_timestamp(Duration::ZERO, Some(duration));
        let duration = format_timestamp(duration);
        format!("{position} / {duration}")
    } else {
        "0:00 / --:--".to_string()
    }
}

pub(super) fn format_position_timestamp(position: Duration, duration: Option<Duration>) -> String {
    let Some(duration) = duration.filter(|duration| duration.as_secs() >= 3600) else {
        return format_timestamp(position);
    };

    format_timestamp_with_hours(position, hour_digits(duration))
}

fn hour_digits(duration: Duration) -> usize {
    ((duration.as_secs() / 3600).max(1)).to_string().len()
}

pub(super) fn progress_pixels(width: u32, position: Duration, duration: Option<Duration>) -> u32 {
    let Some(duration) = duration.filter(|duration| !duration.is_zero()) else {
        return 0;
    };
    let ratio = (position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0);
    (ratio * f64::from(width)).round() as u32
}

pub(super) fn format_timestamp(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;

    if hours > 0 {
        format_timestamp_with_hours(duration, hour_digits(duration))
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_timestamp_with_hours(duration: Duration, hour_width: usize) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;

    format!("{hours:0hour_width$}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
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
}
