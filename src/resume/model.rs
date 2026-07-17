use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use super::identity::normalized_local_path;

#[derive(Clone, Debug)]
pub(crate) struct RestoredPlayback {
    pub(crate) position: Option<Duration>,
    pub(crate) audio: ResumeAudioSelection,
    pub(crate) subtitle: ResumeSubtitleSelection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ResumePlaybackState {
    pub(super) position: Duration,
    pub(super) audio: ResumeAudioSelection,
    pub(super) subtitle: ResumeSubtitleSelection,
}

impl ResumePlaybackState {
    pub(super) fn new() -> Self {
        Self {
            position: Duration::ZERO,
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Unspecified,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResumeAudioSelection {
    Unspecified,
    Disabled,
    Selected {
        stream_index: Option<usize>,
        ordinal: Option<usize>,
        label: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResumeSubtitleSelection {
    Unspecified,
    Off,
    External {
        path: PathBuf,
        relative_path: Option<PathBuf>,
        file_name: Option<PathBuf>,
        ordinal: Option<usize>,
        label: Option<String>,
    },
    Embedded {
        stream_index: Option<usize>,
        ordinal: Option<usize>,
        label: Option<String>,
    },
}

impl ResumeSubtitleSelection {
    pub(crate) fn external(
        path: &Path,
        media_path: &Path,
        ordinal: Option<usize>,
        label: Option<String>,
    ) -> Self {
        let path = normalized_local_path(path);
        let media_path = normalized_local_path(media_path);
        let media_dir = media_path.parent();
        let relative_path =
            media_dir.and_then(|dir| path.strip_prefix(dir).ok().map(Path::to_path_buf));
        let file_name = path.file_name().map(PathBuf::from);
        Self::External {
            path,
            relative_path,
            file_name,
            ordinal,
            label,
        }
    }

    pub(crate) fn external_candidates(&self, media_path: &Path) -> Vec<PathBuf> {
        let Self::External {
            path,
            relative_path,
            file_name,
            ..
        } = self
        else {
            return Vec::new();
        };

        let mut candidates = Vec::new();
        push_unique_path(&mut candidates, path.clone());
        let media_path = normalized_local_path(media_path);
        if let Some(media_dir) = media_path.parent() {
            if let Some(relative_path) = relative_path {
                push_unique_path(&mut candidates, media_dir.join(relative_path));
            }
            if let Some(file_name) = file_name {
                push_unique_path(&mut candidates, media_dir.join(file_name));
            }
        }
        candidates
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|candidate| candidate == &path) {
        paths.push(path);
    }
}
