use super::*;

#[test]
fn parses_short_millisecond_fields() {
    assert_eq!(
        parse_timestamp("00:00:01,5").expect("timestamp should parse"),
        Duration::from_millis(1500)
    );
    assert_eq!(
        parse_timestamp("00:00:01,05").expect("timestamp should parse"),
        Duration::from_millis(1050)
    );
}
#[test]
fn strips_subtitle_markup_without_losing_literal_braces() {
    assert_eq!(strip_srt_markup(r"{\an8}{\i1}ku{\i0}"), "ku");
    assert_eq!(
        strip_srt_markup(r"hello {world} &amp; <i>friends</i>"),
        "hello {world} & friends"
    );
    assert_eq!(strip_srt_markup(r"one\Ntwo\hthree"), "one\ntwo three");
}
