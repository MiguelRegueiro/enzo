use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use super::*;
use crate::decoder_backend::backend_bindings::{
    enzo_audio_seek_leading_silence_samples, enzo_audio_seek_trim_samples,
};

#[test]
fn audio_seek_trimming_discards_early_frames_and_leading_samples() {
    let entirely_early = unsafe {
        enzo_audio_seek_trim_samples(1_000, 0, 1, 1_000, 1_024, 48_000, 1_030_000, 0, 1_024)
    };
    let crossing_target = unsafe {
        enzo_audio_seek_trim_samples(1_000, 0, 1, 1_000, 1_024, 48_000, 1_010_000, 17, 1_041)
    };
    let normalized_start = unsafe {
        enzo_audio_seek_trim_samples(11_400, 1_400, 1, 1_000, 1_024, 48_000, 10_005_000, 0, 1_024)
    };
    let leading_silence =
        unsafe { enzo_audio_seek_leading_silence_samples(11_413, 1_400, 1, 1_000, 10_000_000) };
    let delayed_track_silence =
        unsafe { enzo_audio_seek_leading_silence_samples(500, 0, 1, 1_000, 0) };

    assert_eq!(entirely_early, -1);
    assert_eq!(crossing_target, 497);
    assert_eq!(normalized_start, 240);
    assert_eq!(leading_silence, 624);
    assert_eq!(delayed_track_silence, 24_000);
}

#[test]
fn held_audio_seek_applies_and_prebuffers_before_release_when_pulse_is_available() {
    if Command::new("ffmpeg").arg("-version").output().is_err()
        || !Command::new("pactl")
            .arg("info")
            .output()
            .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let media = std::env::temp_dir().join(format!(
        "enzo-held-audio-seek-test-{}.mkv",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=size=16x16:duration=2:rate=30")
        .args(["-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=2")
        .args([
            "-map", "0:v:0", "-map", "1:a:0", "-c:v", "ffv1", "-c:a", "flac",
        ])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let mut player =
        AudioPlayer::spawn_held_at(&media, None, Duration::from_millis(750), false, true, 100)
            .expect("held audio player should start");
    let generation = player.seek_generation();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !player.seek_applied(generation) || !player.seek_buffered(generation) {
        assert!(
            !player.is_finished().expect("audio thread should not fail"),
            "held audio should not finish before release"
        );
        assert!(
            Instant::now() < deadline,
            "held audio should apply and buffer the seek"
        );
        thread::sleep(Duration::from_millis(2));
    }

    player.release_seek(generation);
    thread::sleep(Duration::from_millis(25));
    player.seek_held(Duration::from_millis(1_250));
    thread::sleep(Duration::from_millis(2));
    let stop_started = Instant::now();
    player.stop().expect("audio player should stop");
    assert!(
        stop_started.elapsed() < Duration::from_secs(1),
        "stopping during a held audio seek should be prompt"
    );

    let mut tail =
        AudioPlayer::spawn_held_at(&media, None, Duration::from_millis(1_990), false, true, 100)
            .expect("held tail audio player should start");
    let tail_generation = tail.seek_generation();
    let tail_deadline = Instant::now() + Duration::from_secs(3);
    while !tail.seek_applied(tail_generation) || !tail.seek_buffered(tail_generation) {
        assert!(
            !tail
                .is_finished()
                .expect("tail audio thread should not fail"),
            "held tail audio should wait for release"
        );
        assert!(
            Instant::now() < tail_deadline,
            "held tail audio should apply and buffer the seek"
        );
        thread::sleep(Duration::from_millis(2));
    }
    tail.release_seek(tail_generation);
    while !tail
        .is_finished()
        .expect("tail audio thread should not fail")
    {
        assert!(
            Instant::now() < tail_deadline,
            "released tail audio should drain and finish"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let _ = std::fs::remove_file(media);
}
