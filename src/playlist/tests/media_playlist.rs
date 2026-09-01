use super::*;

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("enzo-playlist-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).expect("temp dir should be created");
    dir
}

#[test]
fn folder_playlist_uses_natural_video_order() {
    let dir = test_dir("natural-order");
    let ep1 = dir.join("Episode 1.mkv");
    let ep2 = dir.join("Episode 2.mkv");
    let ep10 = dir.join("Episode 10.mkv");
    let ignored = dir.join("Episode 3.srt");
    fs::write(&ep10, "").expect("video should be written");
    fs::write(&ignored, "").expect("subtitle should be written");
    fs::write(&ep1, "").expect("video should be written");
    fs::write(&ep2, "").expect("video should be written");

    let mut playlist = Playlist::from_opened_path(ep2.clone());

    assert!(playlist.controls().previous_available);
    assert!(playlist.controls().next_available);
    assert_eq!(playlist.step(PlaylistStep::Previous), Some(ep1.as_path()));
    assert_eq!(playlist.step(PlaylistStep::Next), Some(ep2.as_path()));
    assert_eq!(playlist.step(PlaylistStep::Next), Some(ep10.as_path()));
    assert_eq!(playlist.step(PlaylistStep::Next), None);

    let view = playlist.view();
    assert_eq!(view.current, 2);
    assert_eq!(
        view.labels.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
        ["Episode 1.mkv", "Episode 2.mkv", "Episode 10.mkv"]
    );
    assert_eq!(playlist.select(0), Some(ep1.as_path()));
    assert_eq!(playlist.select(3), None);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn remote_url_has_no_folder_controls() {
    let playlist = Playlist::from_opened_path(PathBuf::from("https://example.com/video.mp4"));

    assert_eq!(
        playlist.current(),
        Path::new("https://example.com/video.mp4")
    );
    assert_eq!(playlist.controls(), PlaylistControls::default());
}
