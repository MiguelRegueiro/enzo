use super::*;

#[test]
fn normalizes_common_subtitle_language_tags() {
    assert_eq!(normalize_language_tag("eng"), Some("en".to_string()));
    assert_eq!(normalize_language_tag("jpn"), Some("ja".to_string()));
    assert_eq!(normalize_language_tag("cze"), Some("cs".to_string()));
    assert_eq!(normalize_language_tag("dut"), Some("nl".to_string()));
    assert_eq!(normalize_language_tag("rum"), Some("ro".to_string()));
    assert_eq!(
        normalize_language_tag("zh_Hans"),
        Some("zh-Hans".to_string())
    );
    assert_eq!(normalize_language_tag("und"), None);
}

#[test]
fn preserves_well_formed_unknown_language_tags() {
    assert_eq!(normalize_language_tag("ast"), Some("ast".to_string()));
    assert_eq!(
        normalize_language_tag("sr-Latn-RS"),
        Some("sr-Latn-RS".to_string())
    );
    assert_eq!(language_display_name("ast"), "ast");
}

#[test]
fn formats_subtitle_codec_labels() {
    assert_eq!(subtitle_codec_label("subrip"), "SRT");
    assert_eq!(subtitle_codec_label("ass"), "ASS");
    assert_eq!(subtitle_codec_label("hdmv_pgs_subtitle"), "PGS");
}
#[test]
fn infers_language_from_sidecar_filename() {
    assert_eq!(
        language_from_filename(Path::new("movie.jpn.srt")),
        Some("ja".to_string())
    );
    assert_eq!(
        language_from_filename(Path::new("movie.zh.Hans.srt")),
        Some("zh-Hans".to_string())
    );
    assert_eq!(
        language_from_filename(Path::new("movie.sc.ass")),
        Some("zh-Hans".to_string())
    );
    assert_eq!(
        language_from_filename(Path::new("movie.tc.ass")),
        Some("zh-Hant".to_string())
    );
    assert_eq!(language_from_filename(Path::new("movie.srt")), None);
}

#[test]
fn detects_english_subtitle_text_without_filename_language() {
    let text = "\
1
00:00:01,000 --> 00:00:03,000
They all said the tree was rotting.

2
00:00:04,000 --> 00:00:06,000
But I told them it was not.
";

    assert_eq!(detect_text_language(text), Some("en".to_string()));
}

#[test]
fn detects_script_based_subtitle_languages() {
    assert_eq!(
        detect_text_language("Привет, как дела? Это тест."),
        Some("ru".to_string())
    );
    assert_eq!(
        detect_text_language("これは日本語の字幕です。"),
        Some("ja".to_string())
    );
    assert_eq!(
        detect_text_language("이것은 한국어 자막입니다."),
        Some("ko".to_string())
    );
    assert_eq!(
        detect_text_language("这是中文字幕。"),
        Some("zh".to_string())
    );
    assert_eq!(
        detect_text_language("هذه ترجمة عربية."),
        Some("ar".to_string())
    );
}
