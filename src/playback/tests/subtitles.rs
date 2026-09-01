use super::*;

#[test]
fn playback_drop_accepts_subtitle_file() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-app-playback-subtitle-drop-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir(&temp_dir).expect("temp dir should be created");
    let sub_file = temp_dir.join("Movie Signs.eng.ass");
    std::fs::write(&sub_file, "subtitle").expect("subtitle should be written");

    let from_drop = subtitle_path_from_drop_text(&format!("file://{}", sub_file.display()))
        .expect("drop subtitle should parse");

    assert_eq!(from_drop, Some(sub_file));
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn playback_drop_normalizes_duplicate_subtitle_paths() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-app-playback-subtitle-dup-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir(&temp_dir).expect("temp dir should be created");
    let sub_file = temp_dir.join("movie.srt");
    std::fs::write(&sub_file, "subtitle").expect("subtitle should be written");

    let plain = subtitle_path_from_drop_text(&sub_file.display().to_string())
        .expect("plain subtitle should parse")
        .expect("plain subtitle should exist");
    let file_url = subtitle_path_from_drop_text(&format!("file://{}", sub_file.display()))
        .expect("file url subtitle should parse")
        .expect("file url subtitle should exist");

    assert_eq!(
        normalized_subtitle_path(&plain),
        normalized_subtitle_path(&file_url)
    );
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn playback_drop_ignores_non_subtitle_file() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-app-playback-video-drop-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("Movie.mkv");
    std::fs::write(&media, "video").expect("video should be written");

    let from_drop = subtitle_path_from_drop_text(&media.display().to_string())
        .expect("video drop should not error");

    assert_eq!(from_drop, None);
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn pending_tracks_keep_their_picker_label() {
    let tracks = vec![PlaybackSubtitleTrack::pending_embedded(
        "English — Embedded".to_string(),
        Some(0),
    )];
    let labels = build_subtitle_labels(&tracks);

    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].as_ref(), "English — Embedded");
    assert!(active_subtitle_track(&tracks, Some(0)).is_none());
}

#[test]
fn duplicate_subtitle_picker_labels_get_stable_suffixes() {
    let tracks = vec![
        PlaybackSubtitleTrack::pending_embedded("English [SRT]".to_string(), Some(0)),
        PlaybackSubtitleTrack::pending_embedded("English [SRT]".to_string(), Some(1)),
        PlaybackSubtitleTrack::pending_embedded("Spanish [SRT]".to_string(), Some(2)),
        PlaybackSubtitleTrack::pending_embedded("English [SRT]".to_string(), Some(3)),
    ];

    let labels = build_subtitle_labels(&tracks);

    assert_eq!(
        labels.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
        [
            "English [SRT] #1",
            "English [SRT] #2",
            "Spanish [SRT]",
            "English [SRT] #3",
        ]
    );
}

#[test]
fn invalid_media_does_not_invent_pending_embedded_subtitles() {
    let temp_dir = std::env::temp_dir().join(format!(
        "enzo-no-embedded-subtitle-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mp4");
    std::fs::write(&media, b"not really video").expect("media placeholder should be written");

    let loaded = load_initial_subtitle_tracks(&media, &[])
        .expect("subtitle discovery should tolerate videos without subtitle streams");

    assert!(loaded.tracks.is_empty());
    assert!(loaded.embedded_jobs.is_empty());
    assert!(!loaded.restored_external_load_failed);
    assert_eq!(loaded.restored_external_index, None);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn sidecar_stays_before_background_embedded_tracks() {
    let temp_dir = std::env::temp_dir().join(format!("enzo-sidecar-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mkv");
    let subtitle = temp_dir.join("movie.srt");
    std::fs::write(&media, b"not really video").expect("media placeholder should be written");
    std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:01,000\nhello\n")
        .expect("subtitle should be written");

    let (paths, missing) = initial_external_subtitle_paths(&media, None, None);
    let loaded =
        load_initial_subtitle_tracks(&media, &paths).expect("sidecar subtitle should load");

    assert!(!missing);
    assert!(!loaded.restored_external_load_failed);
    assert_eq!(loaded.restored_external_index, None);
    assert_eq!(loaded.tracks.len(), loaded.embedded_jobs.len() + 1);
    assert!(active_subtitle_track(&loaded.tracks, Some(0)).is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn optional_restored_subtitle_failure_does_not_fail_media_load() {
    let temp_dir =
        std::env::temp_dir().join(format!("enzo-optional-subtitle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mkv");
    let subtitle = temp_dir.join("bad.srt");
    std::fs::write(&media, b"not really video").expect("media placeholder should be written");
    std::fs::write(&subtitle, "").expect("subtitle placeholder should be written");

    let loaded = load_initial_subtitle_tracks(
        &media,
        &[InitialSubtitlePath {
            path: subtitle,
            required: false,
            restores_saved_selection: true,
        }],
    )
    .expect("optional restored subtitle failure should be non-fatal");

    assert!(loaded.tracks.is_empty());
    assert!(loaded.embedded_jobs.is_empty());
    assert!(loaded.restored_external_load_failed);
    assert_eq!(loaded.restored_external_index, None);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn catalog_loads_and_reselects_a_dropped_subtitle_without_duplication() {
    let temp_dir =
        std::env::temp_dir().join(format!("enzo-subtitle-catalog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let media = temp_dir.join("movie.mkv");
    let subtitle = temp_dir.join("movie.srt");
    std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:01,000\nhello\n")
        .expect("subtitle should be written");
    let initial = InitialSubtitleLoad {
        tracks: Vec::new(),
        embedded_jobs: Vec::new(),
        restored_external_load_failed: false,
        restored_external_index: None,
    };
    let mut catalog = SubtitleCatalog::new(media, initial, None);
    let drop_text = subtitle.display().to_string();

    assert_eq!(
        catalog.select_from_drop_text(&drop_text),
        DroppedSubtitleSelection::Loaded
    );
    assert_eq!(catalog.tracks().len(), 1);
    assert_eq!(catalog.selected(), Some(0));
    assert_eq!(catalog.labels().len(), 1);
    assert!(catalog.active().is_some());

    catalog.select(None);
    assert_eq!(
        catalog.select_from_drop_text(&drop_text),
        DroppedSubtitleSelection::SelectedExisting
    );
    assert_eq!(catalog.tracks().len(), 1);
    assert_eq!(catalog.selected(), Some(0));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
