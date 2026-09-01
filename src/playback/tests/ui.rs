use super::*;

#[test]
fn forced_media_title_takes_precedence_over_stream_name() {
    let path = Path::new("https://media.example/index-v1-a1.m3u8");

    assert_eq!(
        resolve_media_title(path, Some("Frieren Episode 1")).as_ref(),
        "Frieren Episode 1"
    );
    assert_eq!(
        resolve_media_title(path, Some("葬送のフリーレン Episode 1")).as_ref(),
        "葬送のフリーレン Episode 1"
    );
}

#[test]
fn ordinary_media_titles_keep_the_filename_fallback() {
    assert_eq!(
        resolve_media_title(Path::new("/videos/Movie.mkv"), None).as_ref(),
        "Movie.mkv"
    );
    assert_eq!(
        resolve_media_title(
            Path::new("https://media.example/index-v1-a1.m3u8"),
            Some("")
        )
        .as_ref(),
        "index-v1-a1.m3u8"
    );
}
