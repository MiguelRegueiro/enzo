use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

use anyhow::Result;

use crate::{
    resume::{RestoredPlayback, ResumeSubtitleSelection},
    subtitle::{
        EmbeddedSubtitleStream, SubtitleTrack, embedded_subtitle_streams,
        load_embedded_subtitle_track, sidecar_subtitle_path,
    },
};

use super::super::{cli::validate_subtitle_path, path_input::media_candidates_from_text};

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
    if let Some(path) = external_subtitle_path(media_path, sub_file) {
        push_unique_subtitle_path(&mut paths, path, true, false);
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
        if !stream.is_text() {
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
    tracks
        .iter()
        .map(|track| Arc::<str>::from(track.label.as_str()))
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

fn external_subtitle_path(media_path: &Path, sub_file: Option<&Path>) -> Option<PathBuf> {
    sub_file
        .map(Path::to_path_buf)
        .or_else(|| sidecar_subtitle_path(media_path))
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
mod tests {
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
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-sidecar-load-{}", std::process::id()));
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
}
