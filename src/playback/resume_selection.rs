use std::path::Path;

use crate::{
    audio::AudioTrack,
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
#[path = "tests/resume_selection.rs"]
mod tests;
