use std::path::Path;

use crate::{
    media::AudioTrack,
    resume::{RestoredPlayback, ResumeAudioSelection, ResumeSubtitleSelection, ResumeTracker},
};

use super::{
    engine::AudioChoice,
    subtitles::{PlaybackSubtitleSource, PlaybackSubtitleTrack, normalized_subtitle_path},
};

pub(super) fn resume_available(resume_enabled: bool, source_seekable: bool) -> bool {
    resume_enabled && source_seekable
}

pub(super) fn restore_audio_selection(
    tracks: &[AudioTrack],
    restored: Option<&RestoredPlayback>,
) -> Option<Option<usize>> {
    match &restored?.audio {
        ResumeAudioSelection::Unspecified => None,
        ResumeAudioSelection::Disabled => Some(None),
        ResumeAudioSelection::Selected {
            stream_index,
            ordinal,
            label,
        } => {
            if let Some(stream_index) = stream_index
                && let Some(index) = tracks.iter().position(|track| {
                    track.stream_index() != usize::MAX && track.stream_index() == *stream_index
                })
            {
                return Some(Some(index));
            }
            if let Some(label) = label
                && let Some(index) = tracks.iter().position(|track| track.label() == label)
            {
                return Some(Some(index));
            }
            if let Some(index) = ordinal.and_then(|index| tracks.get(index).map(|_| index)) {
                return Some(Some(index));
            }
            None
        }
    }
}

pub(super) fn restore_subtitle_selection(
    tracks: &[PlaybackSubtitleTrack],
    restored: Option<&RestoredPlayback>,
    restored_external_index: Option<usize>,
) -> Option<Option<usize>> {
    match &restored?.subtitle {
        ResumeSubtitleSelection::Unspecified => None,
        ResumeSubtitleSelection::Off => Some(None),
        ResumeSubtitleSelection::External { path, .. } => {
            if let Some(index) = restored_external_index {
                return Some(Some(index));
            }
            let normalized_path = normalized_subtitle_path(path);
            if let Some(index) = tracks.iter().position(|track| {
                matches!(
                    &track.source,
                    PlaybackSubtitleSource::External { path }
                        if normalized_subtitle_path(path) == normalized_path
                )
            }) {
                return Some(Some(index));
            }
            Some(None)
        }
        ResumeSubtitleSelection::Embedded {
            stream_index,
            ordinal,
            label,
        } => {
            if let Some(stream_index) = stream_index
                && let Some(index) = tracks.iter().position(|track| {
                    matches!(
                        &track.source,
                        PlaybackSubtitleSource::Embedded { stream_index: Some(value) }
                            if value == stream_index
                    )
                })
            {
                return Some(Some(index));
            }
            if let Some(label) = label
                && let Some(index) = tracks.iter().position(|track| {
                    matches!(&track.source, PlaybackSubtitleSource::Embedded { .. })
                        && track.label == *label
                })
            {
                return Some(Some(index));
            }
            Some(ordinal.and_then(|index| {
                tracks
                    .get(index)
                    .filter(|track| {
                        matches!(&track.source, PlaybackSubtitleSource::Embedded { .. })
                    })
                    .map(|_| index)
            }))
        }
    }
}

pub(super) fn selected_audio_choice(
    tracks: &[AudioTrack],
    selected_audio: Option<usize>,
) -> AudioChoice {
    let Some(track) = selected_audio.and_then(|index| tracks.get(index)) else {
        return AudioChoice::Off;
    };
    if track.stream_index() == usize::MAX {
        AudioChoice::Default
    } else {
        AudioChoice::Stream(track.stream_index())
    }
}

pub(super) fn sync_resume_audio(
    resume: &mut ResumeTracker,
    tracks: &[AudioTrack],
    selected_audio: Option<usize>,
) {
    resume.set_audio(saved_audio_selection(tracks, selected_audio));
}

pub(super) fn sync_resume_subtitle(
    resume: &mut ResumeTracker,
    media_path: &Path,
    tracks: &[PlaybackSubtitleTrack],
    selected_subtitle: Option<usize>,
) {
    resume.set_subtitle(saved_subtitle_selection(
        media_path,
        tracks,
        selected_subtitle,
    ));
}

fn saved_audio_selection(
    tracks: &[AudioTrack],
    selected_audio: Option<usize>,
) -> ResumeAudioSelection {
    let Some(index) = selected_audio else {
        return ResumeAudioSelection::Disabled;
    };
    let Some(track) = tracks.get(index) else {
        return ResumeAudioSelection::Disabled;
    };
    ResumeAudioSelection::Selected {
        stream_index: (track.stream_index() != usize::MAX).then_some(track.stream_index()),
        ordinal: Some(index),
        label: Some(track.label().to_string()),
    }
}

fn saved_subtitle_selection(
    media_path: &Path,
    tracks: &[PlaybackSubtitleTrack],
    selected_subtitle: Option<usize>,
) -> ResumeSubtitleSelection {
    let Some(index) = selected_subtitle else {
        return ResumeSubtitleSelection::Off;
    };
    let Some(track) = tracks.get(index) else {
        return ResumeSubtitleSelection::Off;
    };
    match &track.source {
        PlaybackSubtitleSource::External { path } => ResumeSubtitleSelection::external(
            path,
            media_path,
            Some(index),
            Some(track.label.clone()),
        ),
        PlaybackSubtitleSource::Embedded { stream_index } => ResumeSubtitleSelection::Embedded {
            stream_index: *stream_index,
            ordinal: Some(index),
            label: Some(track.label.clone()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use super::*;
    use crate::app::playback::subtitles::{
        initial_external_subtitle_paths, load_initial_subtitle_tracks,
    };
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
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-moved-subtitle-{}", std::process::id()));
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
}
