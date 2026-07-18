use std::{path::Path, sync::Arc};

use crate::{
    media::{AudioTrack, load_audio_tracks},
    resume::RestoredPlayback,
};

use super::{
    engine::AudioChoice,
    resume_selection::{restore_audio_selection, selected_audio_choice},
};

pub(super) struct AudioCatalog {
    tracks: Vec<AudioTrack>,
    labels: Arc<[Arc<str>]>,
    selected: Option<usize>,
}

impl AudioCatalog {
    pub(super) fn load(
        path: &Path,
        source_has_audio: bool,
        restored: Option<&RestoredPlayback>,
    ) -> Self {
        let mut tracks = load_audio_tracks(path);
        if tracks.is_empty() && source_has_audio {
            tracks.push(AudioTrack::default_track());
        }
        let labels = tracks
            .iter()
            .map(|track| Arc::<str>::from(track.label()))
            .collect();
        let selected = restore_audio_selection(&tracks, restored)
            .unwrap_or_else(|| (!tracks.is_empty()).then_some(0));
        Self {
            tracks,
            labels,
            selected,
        }
    }

    pub(super) fn tracks(&self) -> &[AudioTrack] {
        &self.tracks
    }

    pub(super) fn labels(&self) -> Arc<[Arc<str>]> {
        self.labels.clone()
    }

    pub(super) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(super) fn select(&mut self, selected: Option<usize>) {
        debug_assert!(selected.is_none_or(|index| index < self.tracks.len()));
        self.selected = selected;
    }

    pub(super) fn choice(&self) -> AudioChoice {
        selected_audio_choice(&self.tracks, self.selected)
    }

    pub(super) fn is_available(&self) -> bool {
        !self.tracks.is_empty()
    }

    pub(super) fn len(&self) -> usize {
        self.tracks.len()
    }

    pub(super) fn playback_summaries(&self) -> Vec<String> {
        self.tracks
            .iter()
            .map(AudioTrack::playback_summary)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keeps_disabled_and_default_audio_distinct() {
        let track = AudioTrack::default_track();
        let mut catalog = AudioCatalog {
            labels: Arc::from([Arc::<str>::from(track.label())]),
            tracks: vec![track],
            selected: Some(0),
        };

        assert_eq!(catalog.choice(), AudioChoice::Default);
        assert_eq!(catalog.labels()[0].as_ref(), "Default");

        catalog.select(None);

        assert_eq!(catalog.choice(), AudioChoice::Off);
        assert_eq!(catalog.selected(), None);
    }
}
