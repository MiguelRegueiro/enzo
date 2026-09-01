use super::*;
use crate::subtitle::sidecar_subtitle_paths;

#[test]
fn argument_and_drop_resolve_the_same_media_and_sidecar_paths() {
    let temp_dir =
        std::env::temp_dir().join(format!("enzo-media-input-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("Fabricated City.mkv");
    let sidecar = temp_dir.join("Fabricated City.srt");
    std::fs::write(&media, "video").expect("video should be written");
    std::fs::write(&sidecar, "subtitle").expect("subtitle should be written");

    let from_arg = media_path_from_argument(media.clone()).expect("arg media should parse");
    let from_drop =
        media_path_from_drop_text(&media.display().to_string()).expect("drop media should parse");

    assert_eq!(from_drop, from_arg);
    assert_eq!(
        sidecar_subtitle_paths(&from_arg),
        std::slice::from_ref(&sidecar)
    );
    assert_eq!(
        sidecar_subtitle_paths(&from_drop),
        std::slice::from_ref(&sidecar)
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn only_http_and_https_are_remote_media_urls() {
    assert!(is_remote_url_text("http://example.com/video.mp4"));
    assert!(is_remote_url_text("https://example.com/video.mp4"));
    assert!(!is_remote_url_text("ftp://example.com/video.mp4"));
    assert!(!is_remote_url_text("concat:/tmp/one.ts|/tmp/two.ts"));
    assert!(!is_remote_url_text("file:///tmp/video.mp4"));
}

#[test]
fn rejects_unsupported_media_protocols() {
    for input in [
        "ftp://example.com/video.mp4",
        "concat:/tmp/one.ts|/tmp/two.ts",
        "subfile:/tmp/video.mp4",
        "lavfi:testsrc",
    ] {
        let error = media_path_from_argument(PathBuf::from(input))
            .expect_err("unsafe protocol should be rejected");
        assert!(
            error.to_string().contains("unsupported media URL scheme"),
            "unexpected error for {input}: {error}"
        );
    }
}

#[test]
fn existing_local_filename_with_colon_remains_valid() {
    let path = std::env::temp_dir().join(format!(
        "enzo-media-input-colon-test-{}:video.mkv",
        std::process::id()
    ));
    std::fs::write(&path, "video").expect("test file should be written");

    assert_eq!(
        media_path_from_argument(path.clone()).expect("existing file should be accepted"),
        path
    );

    let _ = std::fs::remove_file(path);
}
