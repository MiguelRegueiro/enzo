use super::*;
use std::time::Duration;

#[test]
fn parses_srt_cues() {
    let cues = parse_srt(
        "\
1
00:00:01,500 --> 00:00:03,250
Hello
world
   
2
00:00:04.000 --> 00:00:05.000
<i>Bye</i>
",
    )
    .expect("srt should parse");

    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start, Duration::from_millis(1500));
    assert_eq!(cues[0].end, Duration::from_millis(3250));
    assert_eq!(cues[0].lines, ["Hello", "world"]);
    assert_eq!(cues[1].lines, ["Bye"]);
}

#[test]
fn parses_srt_with_ass_override_tags_left_by_conversion() {
    let cues = parse_srt(
        "\
1
00:00:01,000 --> 00:00:02,000
{\\an8}ku

2
00:00:03,000 --> 00:00:04,000
{\\pos(10,20)}sign {literal}
",
    )
    .expect("srt should parse");

    assert_eq!(cues[0].lines, ["ku"]);
    assert_eq!(cues[1].lines, ["sign {literal}"]);
}
