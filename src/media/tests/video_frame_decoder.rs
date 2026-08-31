use std::{process::Command, sync::atomic::AtomicI32};

use super::*;

#[test]
fn native_decoder_rejects_an_undersized_frame_buffer() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let media =
        std::env::temp_dir().join(format!("enzo-frame-bounds-test-{}.mkv", std::process::id()));
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=size=16x16:duration=0.1:rate=1")
        .args(["-c:v", "ffv1"])
        .arg(&media)
        .status()
        .expect("ffmpeg should run");
    if !status.success() {
        return;
    }

    let mut decoder =
        NativeVideoDecoder::open(&media, 16, 16, 1.0).expect("video decoder should open");
    let mut short_frame = vec![0_u8; 16 * 16 * 3 - 1];
    let error = decoder
        .next_frame(
            &mut short_frame,
            f64::NAN,
            &AtomicI32::new(0),
            &AtomicI32::new(0),
            0,
        )
        .err()
        .expect("undersized output should be rejected");

    assert!(
        error
            .to_string()
            .contains("video frame buffer is too small")
    );
    drop(decoder);
    let _ = std::fs::remove_file(media);
}
