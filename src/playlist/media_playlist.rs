use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::media::is_remote_url_text;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaylistStep {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PlaylistControls {
    pub(crate) previous_available: bool,
    pub(crate) next_available: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PlaylistView {
    pub(crate) controls: PlaylistControls,
    pub(crate) labels: Arc<[Arc<str>]>,
    pub(crate) current: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Playlist {
    entries: Vec<PathBuf>,
    labels: Arc<[Arc<str>]>,
    current: usize,
}

impl Playlist {
    pub(crate) fn from_opened_path(path: PathBuf) -> Self {
        if is_remote_url_text(&path.as_os_str().to_string_lossy()) {
            return Self::single(path);
        }

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut entries = fs::read_dir(parent)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| candidate.is_file())
            .filter(|candidate| is_video_candidate(candidate) || same_path(candidate, &path))
            .collect::<Vec<_>>();

        if entries.is_empty() || !entries.iter().any(|candidate| same_path(candidate, &path)) {
            entries.push(path.clone());
        }

        entries.sort_by(|left, right| compare_paths(left, right));
        entries.dedup_by(|left, right| same_path(left, right));
        let current = entries
            .iter()
            .position(|candidate| same_path(candidate, &path))
            .unwrap_or(0);

        let labels = playlist_labels(&entries);
        Self {
            entries,
            labels,
            current,
        }
    }

    fn single(path: PathBuf) -> Self {
        let labels = playlist_labels(std::slice::from_ref(&path));
        Self {
            entries: vec![path],
            labels,
            current: 0,
        }
    }

    pub(crate) fn current(&self) -> &Path {
        &self.entries[self.current]
    }

    pub(crate) fn controls(&self) -> PlaylistControls {
        PlaylistControls {
            previous_available: self.current > 0,
            next_available: self.current + 1 < self.entries.len(),
        }
    }

    pub(crate) fn view(&self) -> PlaylistView {
        PlaylistView {
            controls: self.controls(),
            labels: Arc::clone(&self.labels),
            current: self.current,
        }
    }

    pub(crate) fn step(&mut self, step: PlaylistStep) -> Option<&Path> {
        match step {
            PlaylistStep::Previous if self.current > 0 => {
                self.current -= 1;
                Some(self.current())
            }
            PlaylistStep::Next if self.current + 1 < self.entries.len() => {
                self.current += 1;
                Some(self.current())
            }
            _ => None,
        }
    }

    pub(crate) fn select(&mut self, index: usize) -> Option<&Path> {
        if index >= self.entries.len() {
            return None;
        }
        self.current = index;
        Some(self.current())
    }
}

fn playlist_labels(entries: &[PathBuf]) -> Arc<[Arc<str>]> {
    entries
        .iter()
        .map(|path| {
            let label = path
                .file_name()
                .filter(|name| !name.is_empty())
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned();
            Arc::<str>::from(label)
        })
        .collect::<Vec<_>>()
        .into()
}

fn is_video_candidate(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "3g2"
            | "3gp"
            | "avi"
            | "flv"
            | "m2ts"
            | "m4v"
            | "mkv"
            | "mov"
            | "mp4"
            | "mpeg"
            | "mpg"
            | "mts"
            | "ogm"
            | "ogv"
            | "ts"
            | "webm"
            | "wmv"
    )
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn compare_paths(left: &Path, right: &Path) -> Ordering {
    natural_compare(
        &left
            .file_name()
            .unwrap_or(left.as_os_str())
            .to_string_lossy(),
        &right
            .file_name()
            .unwrap_or(right.as_os_str())
            .to_string_lossy(),
    )
    .then_with(|| left.as_os_str().cmp(right.as_os_str()))
}

fn natural_compare(left: &str, right: &str) -> Ordering {
    let mut left = left.as_bytes();
    let mut right = right.as_bytes();
    while !left.is_empty() && !right.is_empty() {
        if left[0].is_ascii_digit() && right[0].is_ascii_digit() {
            let (left_number, left_rest) = take_ascii_digits(left);
            let (right_number, right_rest) = take_ascii_digits(right);
            let ordering = compare_ascii_numbers(left_number, right_number);
            if ordering != Ordering::Equal {
                return ordering;
            }
            left = left_rest;
            right = right_rest;
            continue;
        }

        let left_byte = left[0].to_ascii_lowercase();
        let right_byte = right[0].to_ascii_lowercase();
        match left_byte.cmp(&right_byte) {
            Ordering::Equal => {
                left = &left[1..];
                right = &right[1..];
            }
            ordering => return ordering,
        }
    }
    left.len().cmp(&right.len())
}

fn take_ascii_digits(bytes: &[u8]) -> (&[u8], &[u8]) {
    let end = bytes
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(bytes.len());
    bytes.split_at(end)
}

fn compare_ascii_numbers(left: &[u8], right: &[u8]) -> Ordering {
    let left_trimmed = trim_leading_zeroes(left);
    let right_trimmed = trim_leading_zeroes(right);
    left_trimmed
        .len()
        .cmp(&right_trimmed.len())
        .then_with(|| left_trimmed.cmp(right_trimmed))
        .then_with(|| left.len().cmp(&right.len()))
}

fn trim_leading_zeroes(bytes: &[u8]) -> &[u8] {
    let trimmed = bytes
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(bytes.len());
    &bytes[trimmed..]
}

#[cfg(test)]
#[path = "tests/media_playlist.rs"]
mod tests;
