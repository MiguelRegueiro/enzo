use super::*;
use std::time::Duration;

#[test]
fn parses_webvtt_without_external_conversion() {
    let cues = parse_webvtt(
        "\
WEBVTT - Enzo fixture

NOTE this block is ignored
not a cue

intro
00:01.500 --> 00:03.250 align:start position:10%
<i>Hello</i>
world

00:00:04.000 --> 00:00:05.000
Bye
",
    )
    .expect("webvtt should parse");

    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start, Duration::from_millis(1500));
    assert_eq!(cues[0].end, Duration::from_millis(3250));
    assert_eq!(cues[0].lines, ["Hello", "world"]);
    assert_eq!(cues[1].lines, ["Bye"]);
}
