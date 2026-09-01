use std::process::Command;

use super::*;

#[test]
fn source_summary_keeps_original_frame_rate() {
    let info = VideoInfo {
        width: 3840,
        height: 2160,
        fps: 30.0,
        source_fps: 59.94,
        duration: None,
        has_audio: true,
        seekable: true,
        container: Some("matroska,webm".to_string()),
        codec: Some("hevc".to_string()),
        profile: Some("Main 10".to_string()),
        hdr: Some("HDR (PQ)"),
    };

    assert_eq!(
        info.source_summary(),
        "HEVC · Main 10 · 3840×2160 · 59.94 fps · HDR (PQ)"
    );
}

#[test]
fn probe_preserves_source_rate_above_playback_cap_when_ffmpeg_is_available() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media =
        std::env::temp_dir().join(format!("enzo-media-info-test-{}.mkv", std::process::id()));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=size=16x16:duration=0.2:rate=60")
        .args(["-c:v", "ffv1"])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let info = probe_video(&media).expect("generated video should be probed");
    assert!((info.source_fps - 60.0).abs() < 0.01);
    assert_eq!(info.fps, MAX_PLAYBACK_FPS);
    assert_eq!(info.container.as_deref(), Some("matroska,webm"));
    let summary = info.source_summary();
    assert!(summary.starts_with("FFV1"));
    assert!(summary.contains("16×16 · 60 fps"));

    let _ = std::fs::remove_file(media);
}
