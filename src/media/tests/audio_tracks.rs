use std::process::Command;

use super::*;

#[test]
fn formats_probed_audio_track_metadata() {
    let track = audio_track_from_probe(
        AudioTrackProbe {
            stream_index: Some(2),
            codec: Some("aac".to_string()),
            language: Some("Japanese".to_string()),
            title: Some("Japanese 5.1".to_string()),
            channels: Some(6),
            channel_layout: Some("5.1".to_string()),
            sample_rate: Some(48_000),
            default: true,
        },
        0,
    )
    .expect("audio track should parse");

    assert_eq!(track.stream_index(), 2);
    assert_eq!(track.label(), "Japanese 5.1 [Default] [AAC]");
    assert_eq!(
        track.playback_summary(),
        "Source: AAC · 5.1 · 48 kHz | Output: PCM S16 · Stereo · 48 kHz"
    );
}

#[test]
fn audio_track_label_uses_clean_channel_codec_and_flag_groups() {
    let track = audio_track_from_probe(
        AudioTrackProbe {
            stream_index: Some(1),
            codec: Some("eac3".to_string()),
            language: Some("Japanese".to_string()),
            channels: Some(6),
            channel_layout: Some("5.1(side)".to_string()),
            default: true,
            ..AudioTrackProbe::default()
        },
        0,
    )
    .expect("audio track should parse");

    assert_eq!(track.label(), "Japanese 5.1 [Default] [E-AC-3]");
}

#[test]
fn audio_track_label_falls_back_to_track_number() {
    let track = audio_track_from_probe(
        AudioTrackProbe {
            stream_index: Some(7),
            ..AudioTrackProbe::default()
        },
        2,
    )
    .expect("audio track should parse");

    assert_eq!(track.stream_index(), 7);
    assert_eq!(track.label(), "Track 3");
    assert_eq!(
        track.playback_summary(),
        "Output: PCM S16 · Stereo · 48 kHz"
    );
}

#[test]
fn native_probe_reads_audio_track_metadata() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media = std::env::temp_dir().join(format!(
        "enzo-audio-track-probe-test-{}.mkv",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("anullsrc=channel_layout=5.1:sample_rate=48000")
        .args([
            "-t",
            "0.2",
            "-c:a",
            "flac",
            "-metadata:s:a:0",
            "language=jpn",
            "-metadata:s:a:0",
            "title=Japanese 5.1",
            "-disposition:a:0",
            "default",
        ])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let tracks = load_audio_tracks(&media);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].stream_index(), 0);
    assert_eq!(tracks[0].label(), "Japanese 5.1 [Default] [FLAC]");
    assert_eq!(
        tracks[0].playback_summary(),
        "Source: FLAC · 5.1 · 48 kHz | Output: PCM S16 · Stereo · 48 kHz"
    );

    let _ = std::fs::remove_file(media);
}
