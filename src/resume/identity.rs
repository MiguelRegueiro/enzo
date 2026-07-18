use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use super::encoding::{hex_encode, path_to_bytes, stable_hash_hex};

pub(super) const FINGERPRINT_CHUNK_BYTES: u64 = 64 * 1024;
const DURATION_TOLERANCE: Duration = Duration::from_secs(1);

pub(super) const FINGERPRINT_ALGORITHM: &str = "sampled-sha256-v1";
pub(super) const FINGERPRINT_HEX_LEN: usize = 64;

#[derive(Clone, Debug)]
pub(super) struct MediaIdentity {
    pub(super) path_key: Vec<u8>,
    pub(super) metadata: Option<FileMetadata>,
    pub(super) duration: Option<Duration>,
    pub(super) fingerprint_path: Option<PathBuf>,
    pub(super) fingerprint: Option<String>,
}

impl MediaIdentity {
    pub(super) fn for_path(
        path: &Path,
        duration: Option<Duration>,
        include_fingerprint: bool,
    ) -> Self {
        let normalized_path = normalized_media_path(path);
        let metadata = metadata_for_path(&normalized_path);
        let mut identity = Self {
            path_key: path_key_for_media(&normalized_path),
            metadata,
            duration,
            fingerprint_path: normalized_path.is_file().then_some(normalized_path),
            fingerprint: None,
        };
        if include_fingerprint {
            identity.ensure_fingerprint();
        }
        identity
    }

    pub(super) fn ensure_fingerprint(&mut self) {
        if self.fingerprint.is_some() {
            return;
        }
        let Some(path) = self.fingerprint_path.as_deref() else {
            return;
        };
        let Some(len) = self.metadata.as_ref().map(|metadata| metadata.len) else {
            return;
        };
        self.fingerprint = file_fingerprint(path, len).ok().flatten();
    }
}

#[derive(Clone, Debug)]
pub(super) struct FileMetadata {
    pub(super) len: u64,
    pub(super) modified_ms: Option<u64>,
    pub(super) dev: Option<u64>,
    pub(super) ino: Option<u64>,
}

pub(super) fn normalized_local_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn record_name_for_path_key(path_key: &[u8]) -> String {
    stable_hash_hex(path_key)
}

pub(super) fn resume_position(position: Duration, duration: Option<Duration>) -> Option<Duration> {
    if position.is_zero() {
        return None;
    }
    Some(duration.map_or(position, |duration| position.min(duration)))
}

pub(super) fn file_fingerprint(path: &Path, len: u64) -> io::Result<Option<String>> {
    if len == 0 {
        return Ok(None);
    }

    let chunk_len = FINGERPRINT_CHUNK_BYTES.min(len);
    let digest = crate::media::file_fingerprint_digest(path, len, chunk_len)?;
    Ok(Some(hex_encode(&digest)))
}

pub(super) fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(super) fn durations_compatible(record_ms: Option<u64>, duration: Option<Duration>) -> bool {
    match (record_ms, duration) {
        (Some(record_ms), Some(duration)) => {
            duration_millis_close(record_ms, duration_millis_u64(duration))
        }
        _ => true,
    }
}

pub(super) fn duration_millis_close(left: u64, right: u64) -> bool {
    left.abs_diff(right) <= duration_millis_u64(DURATION_TOLERANCE)
}

pub(super) fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(duration_millis_u64)
}

fn metadata_for_path(path: &Path) -> Option<FileMetadata> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(FileMetadata {
        len: metadata.len(),
        modified_ms: metadata.modified().ok().and_then(system_time_millis),
        #[cfg(unix)]
        dev: Some(metadata.dev()),
        #[cfg(not(unix))]
        dev: None,
        #[cfg(unix)]
        ino: Some(metadata.ino()),
        #[cfg(not(unix))]
        ino: None,
    })
}

fn normalized_media_path(path: &Path) -> PathBuf {
    if path.as_os_str().to_string_lossy().contains("://") {
        return path.to_path_buf();
    }
    normalized_local_path(path)
}

pub(super) fn path_key_for_media(path: &Path) -> Vec<u8> {
    path_to_bytes(path)
}
