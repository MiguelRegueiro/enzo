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
const RESUME_END_MARGIN: Duration = Duration::from_secs(1);

pub(super) const FINGERPRINT_ALGORITHM: &str = "sampled-sha256-v1";
pub(super) const FINGERPRINT_HEX_LEN: usize = 64;
const REMOTE_TITLE_KEY_PREFIX: &[u8] = b"remote-title-v1\0";

#[derive(Clone, Debug)]
pub(super) struct MediaIdentity {
    pub(super) path_key: Vec<u8>,
    pub(super) metadata: Option<FileMetadata>,
    pub(super) duration: Option<Duration>,
    pub(super) fingerprint_path: Option<PathBuf>,
    pub(super) fingerprint: Option<String>,
}

impl MediaIdentity {
    #[cfg(test)]
    pub(super) fn for_path(
        path: &Path,
        duration: Option<Duration>,
        include_fingerprint: bool,
    ) -> Self {
        Self::for_media(path, duration, include_fingerprint, None)
    }

    pub(super) fn for_media(
        path: &Path,
        duration: Option<Duration>,
        include_fingerprint: bool,
        remote_title: Option<&str>,
    ) -> Self {
        let normalized_path = normalized_media_path(path);
        let metadata = metadata_for_path(&normalized_path);
        let mut identity = Self {
            path_key: remote_title_path_key(&normalized_path, remote_title)
                .unwrap_or_else(|| path_key_for_media(&normalized_path)),
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
    let Some(duration) = duration else {
        return Some(position);
    };
    if position >= duration.saturating_sub(RESUME_END_MARGIN) {
        return None;
    }
    Some(position.min(duration))
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

pub(super) fn legacy_record_name_for_remote_title(
    path: &Path,
    remote_title: Option<&str>,
) -> Option<String> {
    let normalized_path = normalized_media_path(path);
    remote_title_path_key(&normalized_path, remote_title)?;
    Some(record_name_for_path_key(&path_key_for_media(
        &normalized_path,
    )))
}

fn remote_title_path_key(path: &Path, remote_title: Option<&str>) -> Option<Vec<u8>> {
    let title = remote_title.filter(|title| !title.is_empty())?;
    let origin = normalized_http_origin(path.to_str()?)?;
    let mut key =
        Vec::with_capacity(REMOTE_TITLE_KEY_PREFIX.len() + origin.len() + title.len() + 1);
    key.extend_from_slice(REMOTE_TITLE_KEY_PREFIX);
    key.extend_from_slice(origin.as_bytes());
    key.push(0);
    key.extend_from_slice(title.as_bytes());
    Some(key)
}

fn normalized_http_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let (scheme, default_port) = if scheme.eq_ignore_ascii_case("http") {
        ("http", 80)
    } else if scheme.eq_ignore_ascii_case("https") {
        ("https", 443)
    } else {
        return None;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..authority_end].rsplit('@').next()?;
    if authority.is_empty() || authority.chars().any(char::is_whitespace) {
        return None;
    }

    let (host, port) = split_host_port(authority)?;
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    match port.filter(|port| *port != default_port) {
        Some(port) => Some(format!("{scheme}://{host}:{port}")),
        None => Some(format!("{scheme}://{host}")),
    }
}

fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    if authority.starts_with('[') {
        let closing = authority.find(']')?;
        let host = &authority[..=closing];
        let suffix = &authority[closing + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':')?.parse().ok()?)
        };
        return Some((host, port));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port.parse().ok()?)),
        None => (authority, None),
    };
    (!host.contains([':', '[', ']'])).then_some((host, port))
}
