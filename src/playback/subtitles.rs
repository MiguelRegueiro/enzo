use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

use anyhow::Result;

use crate::{
    media::{media_candidates_from_text, validate_subtitle_path},
    resume::{RestoredPlayback, ResumeSubtitleSelection},
    subtitle::{
        EmbeddedSubtitleStream, SubtitleTrack, embedded_subtitle_streams,
        load_embedded_subtitle_track, sidecar_subtitle_paths,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PlaybackSubtitleSource {
    External { path: PathBuf },
    Embedded { stream_index: Option<usize> },
}

pub(super) struct PlaybackSubtitleTrack {
    pub(super) label: String,
    pub(super) track: Option<SubtitleTrack>,
    pub(super) source: PlaybackSubtitleSource,
}

impl PlaybackSubtitleTrack {
    pub(super) fn loaded_external(path: PathBuf, track: SubtitleTrack) -> Self {
        Self {
            label: track.label().to_string(),
            track: Some(track),
            source: PlaybackSubtitleSource::External { path },
        }
    }

    pub(super) fn pending_embedded(label: String, stream_index: Option<usize>) -> Self {
        Self {
            label,
            track: None,
            source: PlaybackSubtitleSource::Embedded { stream_index },
        }
    }
}

pub(super) struct PendingEmbeddedSubtitle {
    index: usize,
    fallback_index: usize,
    stream: EmbeddedSubtitleStream,
}

pub(super) struct InitialSubtitlePath {
    pub(super) path: PathBuf,
    pub(super) required: bool,
    pub(super) restores_saved_selection: bool,
}

pub(super) struct LoadedEmbeddedSubtitle {
    pub(super) index: usize,
    pub(super) track: Option<SubtitleTrack>,
}

pub(super) struct InitialSubtitleLoad {
    pub(super) tracks: Vec<PlaybackSubtitleTrack>,
    pub(super) embedded_jobs: Vec<PendingEmbeddedSubtitle>,
    pub(super) restored_external_load_failed: bool,
    pub(super) restored_external_index: Option<usize>,
}

pub(super) struct SubtitleCatalog {
    tracks: Vec<PlaybackSubtitleTrack>,
    labels: Arc<[Arc<str>]>,
    selected: Option<usize>,
    external_paths: Vec<(PathBuf, usize)>,
    embedded_loader: mpsc::Receiver<LoadedEmbeddedSubtitle>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DroppedSubtitleSelection {
    Ignored,
    SelectedExisting,
    Loaded,
    Failed,
}

impl SubtitleCatalog {
    pub(super) fn new(
        media_path: PathBuf,
        initial: InitialSubtitleLoad,
        selected: Option<usize>,
    ) -> Self {
        let labels = build_subtitle_labels(&initial.tracks);
        let external_paths = external_subtitle_indices(&initial.tracks);
        let embedded_loader = spawn_embedded_subtitle_loader(media_path, initial.embedded_jobs);
        Self {
            tracks: initial.tracks,
            labels,
            selected,
            external_paths,
            embedded_loader,
        }
    }

    pub(super) fn tracks(&self) -> &[PlaybackSubtitleTrack] {
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

    pub(super) fn active(&self) -> Option<&SubtitleTrack> {
        active_subtitle_track(&self.tracks, self.selected)
    }

    pub(super) fn is_available(&self) -> bool {
        !self.tracks.is_empty()
    }

    pub(super) fn find_external(&self, path: &Path) -> Option<usize> {
        self.external_paths
            .iter()
            .find_map(|(loaded_path, index)| (loaded_path == path).then_some(*index))
    }

    pub(super) fn add_external(&mut self, path: PathBuf, track: SubtitleTrack) -> usize {
        let index = self.tracks.len();
        self.tracks
            .push(PlaybackSubtitleTrack::loaded_external(path.clone(), track));
        self.labels = build_subtitle_labels(&self.tracks);
        self.external_paths.push((path, index));
        self.selected = Some(index);
        index
    }

    pub(super) fn poll_loaded(&self) -> Option<LoadedEmbeddedSubtitle> {
        self.embedded_loader.try_recv().ok()
    }

    pub(super) fn apply_loaded(&mut self, loaded: LoadedEmbeddedSubtitle) -> (usize, bool) {
        let index = loaded.index;
        let loaded_ok = loaded.track.is_some();
        if let Some(slot) = self.tracks.get_mut(index) {
            slot.track = loaded.track;
        }
        (index, loaded_ok)
    }

    pub(super) fn select_from_drop_text(&mut self, text: &str) -> DroppedSubtitleSelection {
        let subtitle_path = match subtitle_path_from_drop_text(text) {
            Ok(Some(path)) => path,
            Ok(None) => return DroppedSubtitleSelection::Ignored,
            Err(_) => return DroppedSubtitleSelection::Failed,
        };
        let key = normalized_subtitle_path(&subtitle_path);
        if let Some(index) = self.find_external(&key) {
            self.select(Some(index));
            return DroppedSubtitleSelection::SelectedExisting;
        }
        let Ok(track) = load_dropped_subtitle_track(&subtitle_path) else {
            return DroppedSubtitleSelection::Failed;
        };
        self.add_external(key, track);
        DroppedSubtitleSelection::Loaded
    }
}

pub(super) fn initial_external_subtitle_paths(
    media_path: &Path,
    sub_file: Option<&Path>,
    restored: Option<&RestoredPlayback>,
) -> (Vec<InitialSubtitlePath>, bool) {
    let mut paths = Vec::new();
    if let Some(path) = sub_file {
        push_unique_subtitle_path(&mut paths, path.to_path_buf(), true, false);
    } else {
        for path in sidecar_subtitle_paths(media_path) {
            push_unique_subtitle_path(&mut paths, path, true, false);
        }
    }

    let mut restored_external_missing = false;
    if let Some(restored) = restored
        && matches!(&restored.subtitle, ResumeSubtitleSelection::External { .. })
    {
        let restored_path = restored
            .subtitle
            .external_candidates(media_path)
            .into_iter()
            .find(|path| loadable_subtitle_path(path));
        if let Some(path) = restored_path {
            push_unique_subtitle_path(&mut paths, path, false, true);
        } else {
            restored_external_missing = true;
        }
    }

    (paths, restored_external_missing)
}

pub(super) fn load_initial_subtitle_tracks(
    media_path: &Path,
    external_paths: &[InitialSubtitlePath],
) -> Result<InitialSubtitleLoad> {
    let mut tracks = Vec::new();
    let mut optional_external_failed = false;
    let mut restored_external_index = None;
    for candidate in external_paths {
        let normalized_path = normalized_subtitle_path(&candidate.path);
        match SubtitleTrack::load(&candidate.path) {
            Ok(track) => {
                let index = tracks.len();
                tracks.push(PlaybackSubtitleTrack::loaded_external(
                    normalized_path,
                    track,
                ));
                if candidate.restores_saved_selection {
                    restored_external_index = Some(index);
                }
            }
            Err(error) if candidate.required => return Err(error),
            Err(_) => optional_external_failed = true,
        }
    }

    let mut jobs = Vec::new();
    for (fallback_index, stream) in embedded_subtitle_streams(media_path)
        .into_iter()
        .enumerate()
    {
        if !stream.is_supported() {
            continue;
        }
        let index = tracks.len();
        let stream_index = stream.subtitle_index();
        tracks.push(PlaybackSubtitleTrack::pending_embedded(
            stream.label(),
            stream_index,
        ));
        jobs.push(PendingEmbeddedSubtitle {
            index,
            fallback_index,
            stream,
        });
    }
    Ok(InitialSubtitleLoad {
        tracks,
        embedded_jobs: jobs,
        restored_external_load_failed: optional_external_failed,
        restored_external_index,
    })
}

pub(super) fn spawn_embedded_subtitle_loader(
    media_path: PathBuf,
    jobs: Vec<PendingEmbeddedSubtitle>,
) -> mpsc::Receiver<LoadedEmbeddedSubtitle> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for job in jobs {
            let track = load_embedded_subtitle_track(&media_path, &job.stream, job.fallback_index)
                .ok()
                .flatten();
            if sender
                .send(LoadedEmbeddedSubtitle {
                    index: job.index,
                    track,
                })
                .is_err()
            {
                break;
            }
        }
    });
    receiver
}

pub(super) fn build_subtitle_labels(tracks: &[PlaybackSubtitleTrack]) -> Arc<[Arc<str>]> {
    let mut totals = HashMap::<&str, usize>::new();
    for track in tracks {
        *totals.entry(track.label.as_str()).or_default() += 1;
    }

    let mut seen = HashMap::<&str, usize>::new();
    tracks
        .iter()
        .map(|track| {
            let total = totals
                .get(track.label.as_str())
                .copied()
                .unwrap_or_default();
            if total <= 1 {
                return Arc::<str>::from(track.label.as_str());
            }
            let count = seen.entry(track.label.as_str()).or_default();
            *count += 1;
            Arc::<str>::from(format!("{} #{}", track.label, count))
        })
        .collect()
}

pub(super) fn external_subtitle_indices(tracks: &[PlaybackSubtitleTrack]) -> Vec<(PathBuf, usize)> {
    tracks
        .iter()
        .enumerate()
        .filter_map(|(index, track)| match &track.source {
            PlaybackSubtitleSource::External { path } => {
                Some((normalized_subtitle_path(path), index))
            }
            PlaybackSubtitleSource::Embedded { .. } => None,
        })
        .collect()
}

pub(super) fn active_subtitle_track(
    tracks: &[PlaybackSubtitleTrack],
    selected_subtitle: Option<usize>,
) -> Option<&SubtitleTrack> {
    selected_subtitle.and_then(|index| tracks.get(index)?.track.as_ref())
}

pub(super) fn normalized_subtitle_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn subtitle_path_from_drop_text(text: &str) -> Result<Option<PathBuf>> {
    for candidate in media_candidates_from_text(text) {
        if !is_supported_subtitle_path(&candidate) {
            continue;
        }
        validate_subtitle_path(&candidate)?;
        return Ok(Some(candidate));
    }
    Ok(None)
}

pub(super) fn load_dropped_subtitle_track(path: &Path) -> Result<SubtitleTrack> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("External");
    Ok(SubtitleTrack::load(path)?.with_label(format!("External — {file_name}")))
}

fn push_unique_subtitle_path(
    paths: &mut Vec<InitialSubtitlePath>,
    path: PathBuf,
    required: bool,
    restores_saved_selection: bool,
) {
    let normalized = normalized_subtitle_path(&path);
    if let Some(existing) = paths
        .iter_mut()
        .find(|candidate| normalized_subtitle_path(&candidate.path) == normalized)
    {
        existing.required |= required;
        existing.restores_saved_selection |= restores_saved_selection;
        return;
    }
    paths.push(InitialSubtitlePath {
        path,
        required,
        restores_saved_selection,
    });
}

fn loadable_subtitle_path(path: &Path) -> bool {
    is_supported_subtitle_path(path) && path.is_file()
}

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn is_supported_subtitle_path(path: &Path) -> bool {
    ["srt", "ass", "ssa", "vtt"]
        .iter()
        .any(|extension| path_extension_is(path, extension))
}

#[cfg(test)]
#[path = "tests/subtitles.rs"]
mod tests;
