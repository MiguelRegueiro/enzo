use super::*;
use std::{process::Command, time::Duration};

#[test]
fn decoded_ass_cues_keep_only_the_event_text() {
    let cue = subtitle_cue_from_decoded(DecodedSubtitleCue {
        start: Duration::from_secs(1),
        end: Duration::from_secs(2),
        kind: DecodedSubtitleTextKind::Ass,
        text: r"0,0,Default,,0,0,0,,{\an8}Hello\Nworld".to_string(),
        bitmap: None,
    })
    .expect("decoded cue should contain text");

    assert_eq!(cue.lines, ["Hello", "world"]);
}

#[test]
fn language_detection_sample_is_bounded_on_utf8_boundaries() {
    let cues = vec![SubtitleCue {
        start: Duration::ZERO,
        end: Duration::from_secs(1),
        lines: vec!["字幕".repeat(LANGUAGE_DETECTION_SAMPLE_BYTES)],
        bitmap: None,
    }];

    let sample = subtitle_language_sample(&cues);

    assert!(sample.len() <= LANGUAGE_DETECTION_SAMPLE_BYTES);
    assert!(sample.is_char_boundary(sample.len()));
}

#[test]
fn decoded_ass_song_fallback_skips_romaji_syllables_but_keeps_translation() {
    let translated = subtitle_cue_from_decoded(DecodedSubtitleCue {
        start: Duration::from_secs(1),
        end: Duration::from_secs(2),
        kind: DecodedSubtitleTextKind::Ass,
        text: r"0,0,ED-E,,0,0,0,,{\pos(960,1050)}Not wanting to hide my eyes".to_string(),
        bitmap: None,
    })
    .expect("translation should remain");
    let romaji = subtitle_cue_from_decoded(DecodedSubtitleCue {
        start: Duration::from_secs(1),
        end: Duration::from_secs(2),
        kind: DecodedSubtitleTextKind::Ass,
        text: r"1,0,ED-R1,,0,0,0,,{\pos(439,54)}k".to_string(),
        bitmap: None,
    });

    assert_eq!(translated.lines, ["Not wanting to hide my eyes"]);
    assert!(romaji.is_none());
}

#[test]
fn recognizes_text_and_bitmap_embedded_subtitle_codecs() {
    let ass = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
        subtitle_index: 0,
        codec: Some("ass".to_string()),
        language: Some("eng".to_string()),
        title: Some("English".to_string()),
        default: true,
        forced: false,
    });
    let pgs = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
        subtitle_index: 1,
        codec: Some("hdmv_pgs_subtitle".to_string()),
        language: Some("eng".to_string()),
        title: None,
        default: false,
        forced: false,
    });

    assert!(ass.is_supported());
    assert_eq!(ass.label(), "English [Default] [ASS]");
    assert!(pgs.is_supported());
}

#[test]
fn embedded_subtitle_labels_use_title_with_language_and_codec_details() {
    let stream = |subtitle_index, language: &str, title: &str| {
        embedded_subtitle_stream_from_info(SubtitleStreamInfo {
            subtitle_index,
            codec: Some("ass".to_string()),
            language: Some(language.to_string()),
            title: Some(title.to_string()),
            default: false,
            forced: false,
        })
    };
    let cc = stream(1, "eng", "English(CC)");
    let portuguese = stream(2, "por", "Portuguese(Brazil)");
    let spanish = stream(3, "spa", "Spanish(Latin_America)");

    assert_eq!(cc.label(), "English [CC] [ASS]");
    assert_eq!(portuguese.label(), "Portuguese (Brazil) [ASS]");
    assert_eq!(spanish.label(), "Spanish (Latin America) [ASS]");
}

#[test]
fn embedded_subtitle_labels_cover_common_untitled_stream_languages() {
    let cases = [
        ("ara", "Arabic [SRT]"),
        ("cze", "Czech [SRT]"),
        ("dan", "Danish [SRT]"),
        ("ger", "German [SRT]"),
        ("gre", "Greek [SRT]"),
        ("fin", "Finnish [SRT]"),
        ("fil", "Filipino [SRT]"),
        ("fre", "French [SRT]"),
        ("heb", "Hebrew [SRT]"),
        ("hrv", "Croatian [SRT]"),
        ("hun", "Hungarian [SRT]"),
        ("ind", "Indonesian [SRT]"),
        ("ita", "Italian [SRT]"),
        ("kor", "Korean [SRT]"),
        ("may", "Malay [SRT]"),
        ("nob", "Norwegian Bokmål [SRT]"),
        ("dut", "Dutch [SRT]"),
        ("pol", "Polish [SRT]"),
        ("por", "Portuguese [SRT]"),
        ("rum", "Romanian [SRT]"),
        ("swe", "Swedish [SRT]"),
        ("tha", "Thai [SRT]"),
        ("tur", "Turkish [SRT]"),
        ("ukr", "Ukrainian [SRT]"),
        ("vie", "Vietnamese [SRT]"),
        ("chi", "Chinese [SRT]"),
    ];

    for (language, expected) in cases {
        let stream = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
            subtitle_index: 0,
            codec: Some("subrip".to_string()),
            language: Some(language.to_string()),
            title: None,
            default: false,
            forced: false,
        });

        assert_eq!(stream.label(), expected, "language code {language}");
    }
}

#[test]
fn embedded_subtitle_labels_preserve_unknown_language_tags() {
    let stream = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
        subtitle_index: 0,
        codec: Some("subrip".to_string()),
        language: Some("ast".to_string()),
        title: None,
        default: false,
        forced: false,
    });

    assert_eq!(stream.label(), "ast [SRT]");
}

#[test]
fn sidecar_path_uses_srt_extension_for_local_files() {
    let temp_dir = std::env::temp_dir().join(format!("enzo-subtitle-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mp4");
    let subtitle = temp_dir.join("movie.srt");
    fs::write(&media, "").expect("media placeholder should be written");
    fs::write(&subtitle, "").expect("subtitle placeholder should be written");

    assert_eq!(sidecar_subtitle_paths(&media), [subtitle]);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn sidecar_path_uses_supported_text_subtitle_extensions() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-subtitle-extension-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mp4");
    let subtitle = temp_dir.join("movie.ass");
    fs::write(&media, "").expect("media placeholder should be written");
    fs::write(&subtitle, "").expect("subtitle placeholder should be written");

    assert_eq!(sidecar_subtitle_paths(&media), [subtitle]);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn sidecar_paths_include_language_suffixed_siblings() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-subtitle-siblings-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mkv");
    let simplified = temp_dir.join("movie.sc.ass");
    let traditional = temp_dir.join("movie.tc.ass");
    let unrelated = temp_dir.join("movie2.ass");
    for path in [&media, &traditional, &unrelated, &simplified] {
        fs::write(path, "").expect("fixture should be written");
    }

    assert_eq!(sidecar_subtitle_paths(&media), [simplified, traditional]);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn external_subtitle_label_keeps_source_and_detected_language() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-external-subtitle-label-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let subtitle = temp_dir.join("movie.srt");
    fs::write(
        &subtitle,
        "1\n00:00:01,000 --> 00:00:03,000\nこれは日本語の字幕です。\n",
    )
    .expect("subtitle fixture should be written");

    let track = SubtitleTrack::load(&subtitle).expect("external subtitle should load");

    assert_eq!(track.language(), Some("ja"));
    assert_eq!(track.label(), "Japanese [External] [SRT]");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn loads_utf16le_external_subtitle_with_bom() {
    let temp_dir =
        std::env::temp_dir().join(format!("enzo-utf16-subtitle-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let subtitle = temp_dir.join("movie.srt");
    let text = "1\r\n00:00:01,000 --> 00:00:03,000\r\nWax on, wax off.\r\n";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&subtitle, bytes).expect("subtitle fixture should be written");

    let track = SubtitleTrack::load(&subtitle).expect("UTF-16 subtitle should load");

    assert_eq!(track.label(), "External [External] [SRT]");
    assert_eq!(
        track.active_lines(Duration::from_secs(1)),
        Some(vec![String::from("Wax on, wax off.")])
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn loads_embedded_srt_subtitle_when_ffmpeg_is_available() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-embedded-subtitle-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let sub = temp_dir.join("subtitle.srt");
    let media = temp_dir.join("embedded.mkv");
    let mut fixture = String::from(
        "1\n00:00:00,000 --> 00:00:01,000\nHello there, this is an embedded subtitle and you are in the test.\n\n",
    );
    for index in 1..130 {
        let start = index * 5;
        let end = start + 4;
        fixture.push_str(&format!(
            "{}\n00:00:00,{start:03} --> 00:00:00,{end:03}\nCue {index}\n\n",
            index + 1
        ));
    }
    fs::write(&sub, fixture).expect("subtitle fixture should be written");

    let status = Command::new("ffmpeg")
        .arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("color=size=16x16:duration=1:rate=1")
        .arg("-f")
        .arg("srt")
        .arg("-i")
        .arg(&sub)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:s:0")
        .arg("-c:v")
        .arg("ffv1")
        .arg("-c:s")
        .arg("srt")
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        let _ = fs::remove_dir_all(&temp_dir);
        return;
    }

    let track = load_embedded_subtitle_tracks(&media)
        .expect("embedded subtitle load should not error")
        .into_iter()
        .next()
        .expect("embedded subtitle should be found");
    assert_eq!(track.cues.len(), 130);
    assert_eq!(track.language(), Some("en"));
    assert_eq!(
        track.active_lines(Duration::ZERO),
        Some(vec![String::from(
            "Hello there, this is an embedded subtitle and you are in the test.",
        )])
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn loads_embedded_ass_subtitle_without_srt_karaoke_flattening() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-embedded-ass-subtitle-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir(&temp_dir).expect("temp dir should be created");
    let sub = temp_dir.join("subtitle.ass");
    let media = temp_dir.join("embedded-ass.mkv");
    fs::write(
            &sub,
            "\
[Script Info]
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an8}Whole sentence, not syllable
",
        )
        .expect("subtitle fixture should be written");

    let status = Command::new("ffmpeg")
        .arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("color=size=16x16:duration=1:rate=1")
        .arg("-i")
        .arg(&sub)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:s:0")
        .arg("-c:v")
        .arg("ffv1")
        .arg("-c:s")
        .arg("ass")
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        let _ = fs::remove_dir_all(&temp_dir);
        return;
    }

    let track = load_embedded_subtitle_tracks(&media)
        .expect("embedded subtitle load should not error")
        .into_iter()
        .next()
        .expect("embedded subtitle should be found");
    assert_eq!(
        track.active_lines(Duration::ZERO),
        Some(vec![String::from("Whole sentence, not syllable")])
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
