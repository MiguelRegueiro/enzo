use std::{path::PathBuf, time::Duration};

use super::*;
use crate::playback::subtitles::{initial_external_subtitle_paths, load_initial_subtitle_tracks};
use crate::resume::ResumeSubtitleSelection;

#[test]
fn resume_requires_user_enablement_and_seekable_media() {
    assert!(resume_available(true, true));
    assert!(!resume_available(false, true));
    assert!(!resume_available(true, false));
}

#[test]
fn default_audio_stream_and_disabled_audio_remain_distinct() {
    let tracks = vec![AudioTrack::default_track()];

    assert_eq!(
        selected_audio_choice(&tracks, Some(0)),
        AudioChoice::Default
    );
    assert_eq!(selected_audio_choice(&tracks, None), AudioChoice::Off);
}

#[test]
fn unresolved_saved_audio_falls_back_to_default_audio() {
    let tracks = vec![AudioTrack::default_track()];
    let restored = RestoredPlayback {
        position: None,
        audio: ResumeAudioSelection::Selected {
            stream_index: Some(42),
            ordinal: Some(99),
            label: Some("missing".to_string()),
        },
        subtitle: ResumeSubtitleSelection::Unspecified,
    };

    assert_eq!(restore_audio_selection(&tracks, Some(&restored)), None);
}

#[test]
fn moved_external_subtitle_restores_the_resolved_candidate() {
    let temp_dir = std::env::temp_dir().join(format!("enzo-moved-subtitle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let old_dir = temp_dir.join("old");
    let old_sub_dir = old_dir.join("subs");
    std::fs::create_dir_all(&old_sub_dir).expect("old subtitle directory should be created");
    let old_media = old_dir.join("movie.mkv");
    let old_subtitle = old_sub_dir.join("english.srt");
    std::fs::write(&old_media, b"not really video").expect("media should be written");
    std::fs::write(&old_subtitle, "1\n00:00:00,000 --> 00:00:01,000\nhello\n")
        .expect("subtitle should be written");
    let restored = RestoredPlayback {
        position: Some(Duration::from_secs(10)),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::external(
            &old_subtitle,
            &old_media,
            Some(99),
            Some("English".to_string()),
        ),
    };
    let new_dir = temp_dir.join("new");
    std::fs::rename(&old_dir, &new_dir).expect("media directory should move");
    let new_media = new_dir.join("movie.mkv");

    let (paths, missing) = initial_external_subtitle_paths(&new_media, None, Some(&restored));
    let loaded =
        load_initial_subtitle_tracks(&new_media, &paths).expect("moved subtitle should load");
    let selected = restore_subtitle_selection(
        &loaded.tracks,
        Some(&restored),
        loaded.restored_external_index,
    );

    assert!(!missing);
    assert!(!loaded.restored_external_load_failed);
    assert_eq!(loaded.restored_external_index, Some(0));
    assert_eq!(selected, Some(Some(0)));
    assert!(matches!(
        &loaded.tracks[0].source,
        PlaybackSubtitleSource::External { path }
            if path == &normalized_subtitle_path(&new_dir.join("subs/english.srt"))
    ));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn missing_external_subtitle_stays_off() {
    let tracks = vec![PlaybackSubtitleTrack::pending_embedded(
        "English".to_string(),
        Some(3),
    )];
    let restored = RestoredPlayback {
        position: Some(Duration::from_secs(10)),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::External {
            path: PathBuf::from("/missing/english.srt"),
            relative_path: None,
            file_name: Some(PathBuf::from("english.srt")),
            ordinal: Some(0),
            label: Some("English".to_string()),
        },
    };

    assert_eq!(
        restore_subtitle_selection(&tracks, Some(&restored), None),
        Some(None)
    );
}

#[test]
fn embedded_subtitle_ordinal_does_not_select_an_external_track() {
    let tracks = vec![PlaybackSubtitleTrack {
        label: "External".to_string(),
        track: None,
        source: PlaybackSubtitleSource::External {
            path: PathBuf::from("/tmp/external.srt"),
        },
    }];
    let restored = RestoredPlayback {
        position: Some(Duration::from_secs(10)),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::Embedded {
            stream_index: None,
            ordinal: Some(0),
            label: None,
        },
    };

    assert_eq!(
        restore_subtitle_selection(&tracks, Some(&restored), None),
        Some(None)
    );
}
