use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

use crate::media_input::is_remote_url_text;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlaylistStep {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PlaylistControls {
    pub(super) previous_available: bool,
    pub(super) next_available: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Playlist {
    entries: Vec<PathBuf>,
    current: usize,
}

impl Playlist {
    pub(super) fn from_opened_path(path: PathBuf) -> Self {
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

        Self { entries, current }
    }

    fn single(path: PathBuf) -> Self {
        Self {
            entries: vec![path],
            current: 0,
        }
    }

    pub(super) fn current(&self) -> &Path {
        &self.entries[self.current]
    }

    pub(super) fn controls(&self) -> PlaylistControls {
        PlaylistControls {
            previous_available: self.current > 0,
            next_available: self.current + 1 < self.entries.len(),
        }
    }

    pub(super) fn step(&mut self, step: PlaylistStep) -> Option<&Path> {
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
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("enzo-playlist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn folder_playlist_uses_natural_video_order() {
        let dir = test_dir("natural-order");
        let ep1 = dir.join("Episode 1.mkv");
        let ep2 = dir.join("Episode 2.mkv");
        let ep10 = dir.join("Episode 10.mkv");
        let ignored = dir.join("Episode 3.srt");
        fs::write(&ep10, "").expect("video should be written");
        fs::write(&ignored, "").expect("subtitle should be written");
        fs::write(&ep1, "").expect("video should be written");
        fs::write(&ep2, "").expect("video should be written");

        let mut playlist = Playlist::from_opened_path(ep2.clone());

        assert!(playlist.controls().previous_available);
        assert!(playlist.controls().next_available);
        assert_eq!(playlist.step(PlaylistStep::Previous), Some(ep1.as_path()));
        assert_eq!(playlist.step(PlaylistStep::Next), Some(ep2.as_path()));
        assert_eq!(playlist.step(PlaylistStep::Next), Some(ep10.as_path()));
        assert_eq!(playlist.step(PlaylistStep::Next), None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_url_has_no_folder_controls() {
        let playlist = Playlist::from_opened_path(PathBuf::from("https://example.com/video.mp4"));

        assert_eq!(
            playlist.current(),
            Path::new("https://example.com/video.mp4")
        );
        assert_eq!(playlist.controls(), PlaylistControls::default());
    }
}
