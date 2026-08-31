use super::*;
use crate::subtitle::track::SubtitleTrack;

#[test]
fn parses_ass_dialogues_and_overlapping_lines() {
    let cues = parse_ass(
            "\
[Script Info]
Title: test

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.56,0:00:05.27,OP-E1,,0,0,0,fx,{\\pos(960,1050)\\clip(1,2,3,4)}Surpass a fiction that nobody knows..
Dialogue: 0,0:00:00.60,0:00:01.60,OP-R1,,0,0,0,fx,{\\an5\\move(724.5,30,724.5,75,0,400)}m
Dialogue: 0,0:00:00.70,0:00:01.70,SeriesTitle,,0,0,0,,{\\p1}m 0 0 l 100 0 100 100 0 100
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\an8}Normal line\\Nsecond half
",
        )
        .expect("ass should parse");

    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].lines, ["Normal line", "second half"]);

    let track = SubtitleTrack::from_cues(cues, None, String::from("Subtitles"));
    assert_eq!(
        track.active_lines(Duration::from_millis(1200)),
        Some(vec![
            String::from("Normal line"),
            String::from("second half")
        ])
    );
}

#[test]
fn ass_song_fallback_skips_romaji_syllables_but_keeps_translation() {
    let cues = parse_ass(
            "\
[Script Info]
Title: test

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:22:39.29,0:22:44.88,ED-E,,0,0,0,,{\\pos(960,1050)\\3c&HAE641B&\\blur6}Not wanting to hide my eyes from sad happenings,
Dialogue: 0,0:22:39.35,0:22:40.00,ED-R1,,0,0,0,,{\\pos(439,54)}k
Dialogue: 0,0:22:39.38,0:22:40.03,ED-R1,,0,0,0,,{\\pos(467,54)}a
Dialogue: 0,0:22:39.41,0:22:40.06,ED-R1,,0,0,0,,{\\pos(495,54)}n
",
        )
        .expect("ass should parse");
    let track = SubtitleTrack::from_cues(cues, None, String::from("Subtitles"));

    assert_eq!(
        track.active_lines(Duration::from_millis(22 * 60 * 1000 + 40 * 1000)),
        Some(vec![String::from(
            "Not wanting to hide my eyes from sad happenings,"
        )])
    );
}

#[test]
fn ass_skips_transient_positioned_animation_slices_but_keeps_stable_text() {
    let cues = parse_ass(
            "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 1,0:00:01.00,0:00:01.04,General Title,,0,0,0,,{\\pos(490,907)}A valuabq{\\alpha&HFF&}e herb\n\
Dialogue: 1,0:00:01.04,0:00:01.08,General Title,,0,0,0,,{\\pos(490,907)}A valuable{\\alpha&HFF&} herb\n\
Dialogue: 2,0:00:01.00,0:00:03.00,General Title,,0,0,0,,{\\pos(490,907)}A valuable herb\n\
Dialogue: 0,0:00:04.00,0:00:04.04,Default,,0,0,0,,Brief dialogue\n",
        )
        .expect("ass should parse");

    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].lines, ["A valuable herb"]);
    assert_eq!(cues[1].lines, ["Brief dialogue"]);
}
