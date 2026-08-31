use std::path::PathBuf;

use super::*;

#[test]
fn parses_plain_path_drop() {
    assert_eq!(
        media_candidates_from_text("/tmp/video.mp4").first(),
        Some(&PathBuf::from("/tmp/video.mp4"))
    );
}

#[test]
fn parses_shell_escaped_path_drop() {
    assert!(
        media_candidates_from_text("/tmp/video\\ file.mp4")
            .contains(&PathBuf::from("/tmp/video file.mp4"))
    );
}

#[test]
fn parses_file_url_drop() {
    assert_eq!(
        media_candidates_from_text("file:///tmp/video%20file.mp4").first(),
        Some(&PathBuf::from("/tmp/video file.mp4"))
    );
}

#[test]
fn keeps_remote_url_drop() {
    assert_eq!(
        media_candidates_from_text("https://example.com/video.mp4").first(),
        Some(&PathBuf::from("https://example.com/video.mp4"))
    );
}
