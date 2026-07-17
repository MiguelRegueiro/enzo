use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

use super::{
    identity::{MediaIdentity, record_name_for_path_key, resume_position},
    model::{RestoredPlayback, ResumeAudioSelection, ResumePlaybackState, ResumeSubtitleSelection},
    record::ResumeRecord,
    store::{Durability, ResumeStore},
};

const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const MINIMUM_RESUME_POSITION: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
pub(super) enum SaveMode {
    CleanupBelowThreshold,
    PreserveBelowThreshold,
}

pub(crate) struct ResumeTracker {
    pub(super) store: Option<ResumeStore>,
    pub(super) identity: MediaIdentity,
    pub(super) record_name: String,
    pub(super) loaded_record_name: Option<String>,
    pub(super) restored: Option<RestoredPlayback>,
    pub(super) state: ResumePlaybackState,
    pub(super) last_saved_state: Option<ResumePlaybackState>,
    pub(super) last_checkpoint_at: Instant,
    pub(super) last_error: Option<io::Error>,
    pub(super) finished: bool,
}

impl ResumeTracker {
    pub(crate) fn open(
        media_path: &Path,
        duration: Option<Duration>,
        resume_enabled: bool,
    ) -> Self {
        if !resume_enabled {
            return Self::disabled(media_path, duration);
        }
        let Some(store) = ResumeStore::default() else {
            return Self::disabled(media_path, duration);
        };
        let mut last_error = store.housekeep().err();
        let (identity, loaded) = match store.load(media_path, duration) {
            Ok(loaded) => loaded,
            Err(error) => {
                if last_error.is_none() {
                    last_error = Some(error);
                }
                (MediaIdentity::for_path(media_path, duration, false), None)
            }
        };
        let record_name = record_name_for_path_key(&identity.path_key);
        let (loaded_record_name, restored) = loaded
            .map(|loaded| {
                (
                    Some(loaded.record_name),
                    Some(RestoredPlayback {
                        position: resume_position(loaded.record.position, duration),
                        audio: loaded.record.audio,
                        subtitle: loaded.record.subtitle,
                    }),
                )
            })
            .unwrap_or((None, None));

        Self {
            store: Some(store),
            identity,
            record_name,
            loaded_record_name,
            restored,
            state: ResumePlaybackState::new(),
            last_saved_state: None,
            last_checkpoint_at: Instant::now(),
            last_error,
            finished: false,
        }
    }

    pub(crate) fn clear_all() -> io::Result<usize> {
        let Some(store) = ResumeStore::default() else {
            return Ok(0);
        };
        store.clear_all()
    }

    pub(crate) fn restored(&self) -> Option<&RestoredPlayback> {
        self.restored.as_ref()
    }

    pub(crate) fn set_position(&mut self, position: Duration) {
        self.state.position = position;
    }

    pub(crate) fn set_audio(&mut self, audio: ResumeAudioSelection) {
        self.state.audio = audio;
    }

    pub(crate) fn set_subtitle(&mut self, subtitle: ResumeSubtitleSelection) {
        self.state.subtitle = subtitle;
    }

    pub(crate) fn maybe_checkpoint(&mut self, now: Instant) {
        if self.finished
            || now.saturating_duration_since(self.last_checkpoint_at) < CHECKPOINT_INTERVAL
        {
            return;
        }
        self.last_checkpoint_at = now;
        if self
            .last_saved_state
            .as_ref()
            .is_some_and(|saved| saved == &self.state)
        {
            return;
        }
        if let Err(error) =
            self.save_current(SaveMode::CleanupBelowThreshold, Durability::Checkpoint)
        {
            self.last_error = Some(error);
        }
    }

    pub(crate) fn take_error(&mut self) -> Option<io::Error> {
        self.last_error.take()
    }

    pub(crate) fn save_now(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.save_current(SaveMode::CleanupBelowThreshold, Durability::Final)?;
        self.finished = true;
        Ok(())
    }

    pub(crate) fn clear(&mut self) -> io::Result<()> {
        let result = self
            .store
            .as_ref()
            .map(|store| self.remove_saved_records(store, Durability::Final))
            .unwrap_or(Ok(()));
        self.finished = true;
        result
    }

    pub(super) fn save_current(
        &mut self,
        mode: SaveMode,
        durability: Durability,
    ) -> io::Result<()> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };
        if self.state.position < MINIMUM_RESUME_POSITION {
            if matches!(mode, SaveMode::PreserveBelowThreshold) {
                return Ok(());
            }
            self.remove_saved_records(&store, durability)?;
            self.last_saved_state = Some(self.state.clone());
            return Ok(());
        }
        self.identity.ensure_fingerprint();
        let record = ResumeRecord::from_state(&self.identity, self.state.clone());
        store.write_record(&self.record_name, &record, durability)?;
        if let Some(loaded_record_name) = self
            .loaded_record_name
            .as_deref()
            .filter(|name| *name != self.record_name.as_str())
            .map(str::to_string)
        {
            store.remove_record(&loaded_record_name)?;
            self.loaded_record_name = None;
            if matches!(durability, Durability::Final) {
                store.sync_dir()?;
            }
        }
        self.last_saved_state = Some(self.state.clone());
        Ok(())
    }

    fn disabled(media_path: &Path, duration: Option<Duration>) -> Self {
        let identity = MediaIdentity::for_path(media_path, duration, false);
        let record_name = record_name_for_path_key(&identity.path_key);
        Self {
            store: None,
            identity,
            record_name,
            loaded_record_name: None,
            restored: None,
            state: ResumePlaybackState::new(),
            last_saved_state: None,
            last_checkpoint_at: Instant::now(),
            last_error: None,
            finished: true,
        }
    }

    fn remove_saved_records(&self, store: &ResumeStore, durability: Durability) -> io::Result<()> {
        store.remove_record(&self.record_name)?;
        if let Some(loaded_record_name) = self
            .loaded_record_name
            .as_deref()
            .filter(|name| *name != self.record_name)
        {
            store.remove_record(loaded_record_name)?;
        }
        if matches!(durability, Durability::Final) {
            store.sync_dir()?;
        }
        store.remove_empty_dir()
    }
}

impl Drop for ResumeTracker {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.save_current(SaveMode::PreserveBelowThreshold, Durability::Checkpoint);
        }
    }
}
