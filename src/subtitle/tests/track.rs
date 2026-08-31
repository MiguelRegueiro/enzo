use super::*;
use crate::subtitle::srt::parse_srt;

#[test]
fn active_lines_uses_current_position() {
    let track = SubtitleTrack::from_cues(
        parse_srt(
            "\
1
00:00:01,000 --> 00:00:02,000
One
",
        )
        .expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );

    assert!(track.active_lines(Duration::from_millis(999)).is_none());
    assert_eq!(
        track.active_lines(Duration::from_millis(1000)),
        Some(vec![String::from("One")])
    );
    assert!(track.active_lines(Duration::from_millis(2000)).is_none());
}

#[test]
fn active_lines_preserves_two_line_order() {
    let track = SubtitleTrack::from_cues(
        parse_srt(
            "\
1
00:00:01,000 --> 00:00:02,000
First line
Second line
",
        )
        .expect("srt should parse"),
        None,
        String::from("Subtitles"),
    );

    assert_eq!(
        track.active_lines(Duration::from_millis(1000)),
        Some(vec![
            String::from("First line"),
            String::from("Second line"),
        ])
    );
}

#[test]
fn active_lines_caps_dense_ass_fallbacks_to_longest_lines() {
    let track = SubtitleTrack::from_cues(
        vec![
            SubtitleCue {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
                lines: vec![String::from("A")],
                bitmap: None,
            },
            SubtitleCue {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
                lines: vec![String::from("long translated sentence")],
                bitmap: None,
            },
            SubtitleCue {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
                lines: vec![String::from("medium label")],
                bitmap: None,
            },
            SubtitleCue {
                start: Duration::ZERO,
                end: Duration::from_secs(1),
                lines: vec![String::from("another useful line")],
                bitmap: None,
            },
        ],
        None,
        String::from("Subtitles"),
    );

    assert_eq!(
        track.active_lines(Duration::ZERO),
        Some(vec![
            String::from("long translated sentence"),
            String::from("another useful line"),
            String::from("medium label"),
        ])
    );
}

#[test]
fn text_timeline_collapses_dense_duplicate_layers() {
    let cues = (0..512)
        .map(|offset| SubtitleCue {
            start: Duration::from_millis(offset),
            end: Duration::from_millis(offset + 100),
            lines: vec![String::from("Stable visible subtitle")],
            bitmap: None,
        })
        .collect::<Vec<_>>();

    let track = SubtitleTrack::from_cues(cues, None, String::from("Subtitles"));

    assert_eq!(track.text_timeline.len(), 1);
    assert_eq!(track.text_timeline[0].start, Duration::ZERO);
    assert_eq!(track.text_timeline[0].end, Duration::from_millis(611));
    assert_eq!(
        track.active_lines(Duration::from_millis(500)),
        Some(vec![String::from("Stable visible subtitle")])
    );
}
