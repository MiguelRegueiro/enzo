use std::{fs, process::Command};

use super::*;

#[test]
fn rapid_video_seeks_publish_only_the_latest_generation() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media = std::env::temp_dir().join(format!(
        "enzo-rapid-video-seek-test-{}.mkv",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x180:duration=8:rate=30")
        .args(["-c:v", "mpeg4", "-g", "240"])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let mut decoder = VideoDecoder::spawn_at(&media, 64, 36, 30.0, Duration::ZERO, true)
        .expect("video decoder should start");
    let superseded = decoder.seek(Duration::from_millis(7_500));
    thread::sleep(Duration::from_millis(2));
    let latest = decoder.seek(Duration::from_millis(1_000));
    let deadline = Instant::now() + Duration::from_secs(3);
    let latest_pts = loop {
        if let Some(pts) = decoder.seek_frame(latest) {
            break pts;
        }
        assert!(
            Instant::now() < deadline,
            "latest seek frame should become ready"
        );
        thread::sleep(Duration::from_millis(2));
    };

    assert!(decoder.seek_frame(superseded).is_none());
    assert!(latest_pts >= Duration::from_millis(950));
    assert!(latest_pts < Duration::from_millis(1_100));
    decoder.stop().expect("video decoder should stop");
    let _ = std::fs::remove_file(media);
}

#[test]
fn preview_video_seek_publishes_keyframe_without_exact_catchup() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media = std::env::temp_dir().join(format!(
        "enzo-preview-video-seek-test-{}.mkv",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x180:duration=8:rate=30")
        .args(["-c:v", "mpeg4", "-g", "240"])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let mut decoder = VideoDecoder::spawn_at(&media, 64, 36, 30.0, Duration::ZERO, true)
        .expect("video decoder should start");
    let generation = decoder.preview_seek(Duration::from_millis(7_500));
    let deadline = Instant::now() + Duration::from_secs(3);
    let pts = loop {
        if let Some(pts) = decoder.seek_frame(generation) {
            break pts;
        }
        assert!(
            Instant::now() < deadline,
            "preview seek frame should become ready"
        );
        thread::sleep(Duration::from_millis(2));
    };

    assert!(pts < Duration::from_millis(7_000));
    decoder.stop().expect("video decoder should stop");
    let _ = std::fs::remove_file(media);
}

#[test]
fn hls_seeks_recover_when_segment_timestamps_overshoot() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("enzo-hls-video-seek-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).expect("HLS fixture directory should be created");
    let playlist = directory.join("index.m3u8");
    let segment_pattern = directory.join("segment%02d.ts");
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=64x64:duration=24:rate=25")
        .args(["-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=24")
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "mpeg2video",
            "-g",
            "100",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
            "-f",
            "hls",
            "-hls_time",
            "4",
            "-hls_list_size",
            "0",
            "-hls_playlist_type",
            "vod",
            "-hls_segment_filename",
        ])
        .arg(&segment_pattern)
        .arg(&playlist)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        let _ = fs::remove_dir_all(directory);
        return;
    }

    // Make the playlist timeline slightly later than the packet timeline.
    // Affected HLS demuxers then discard the requested segment's keyframe
    // and return the following segment instead.
    let contents = fs::read_to_string(&playlist).expect("HLS playlist should be readable");
    let skewed = contents
        .lines()
        .map(|line| {
            if line.starts_with("#EXTINF:") {
                "#EXTINF:4.100000,"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&playlist, format!("{skewed}\n")).expect("skewed HLS playlist should be writable");

    let mut decoder = VideoDecoder::spawn_at(&playlist, 64, 64, 25.0, Duration::ZERO, true)
        .expect("HLS video decoder should start");
    for target in [Duration::from_millis(12_500), Duration::from_millis(7_500)] {
        let generation = decoder.seek(target);
        let deadline = Instant::now() + Duration::from_secs(5);
        let pts = loop {
            if let Some(pts) = decoder.seek_frame(generation) {
                break pts;
            }
            assert!(Instant::now() < deadline, "HLS seek should finish");
            thread::sleep(Duration::from_millis(2));
        };
        assert!(
            pts.abs_diff(target) <= Duration::from_millis(60),
            "HLS seek to {target:?} returned {pts:?}"
        );
    }

    let preview_target = Duration::from_millis(12_500);
    let preview_generation = decoder.preview_seek(preview_target);
    let preview_deadline = Instant::now() + Duration::from_secs(5);
    let preview_pts = loop {
        if let Some(pts) = decoder.seek_frame(preview_generation) {
            break pts;
        }
        assert!(
            Instant::now() < preview_deadline,
            "HLS preview seek should finish"
        );
        thread::sleep(Duration::from_millis(2));
    };
    assert!(
        preview_pts <= preview_target,
        "HLS preview must not overshoot its target"
    );

    decoder.stop().expect("HLS video decoder should stop");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn video_seek_normalizes_nonzero_stream_start_time() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media = std::env::temp_dir().join(format!(
        "enzo-video-start-time-test-{}.ts",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=64x64:duration=5:rate=30")
        .args(["-c:v", "mpeg2video", "-g", "30", "-f", "mpegts"])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let mut decoder = VideoDecoder::spawn_at(&media, 64, 64, 30.0, Duration::ZERO, true)
        .expect("video decoder should start");
    let generation = decoder.seek(Duration::from_millis(2_400));
    let deadline = Instant::now() + Duration::from_secs(3);
    let pts = loop {
        if let Some(pts) = decoder.seek_frame(generation) {
            break pts;
        }
        assert!(
            Instant::now() < deadline,
            "normalized seek frame should become ready"
        );
        thread::sleep(Duration::from_millis(2));
    };

    assert!(pts >= Duration::from_millis(2_350));
    assert!(pts < Duration::from_millis(3_100));
    decoder.stop().expect("video decoder should stop");
    let _ = std::fs::remove_file(media);
}
