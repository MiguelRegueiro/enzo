use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    env,
    ffi::OsString,
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use super::{
    identity::{MediaIdentity, record_name_for_path_key},
    record::ResumeRecord,
};

pub(super) const MAX_RECORD_BYTES: u64 = 64 * 1024;
pub(super) const MAX_RENAME_CANDIDATES: usize = 512;
pub(super) const LEGACY_INDEX_FILE: &str = "index";

const STALE_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);
static TMP_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(super) enum Durability {
    Checkpoint,
    Final,
}

#[derive(Clone)]
pub(super) struct ResumeStore {
    pub(super) dir: PathBuf,
}

impl ResumeStore {
    pub(super) fn default() -> Option<Self> {
        let state_home = resume_state_home(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))?;
        Some(Self {
            dir: state_home.join("enzo/watch_later"),
        })
    }

    #[cfg(test)]
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub(super) fn load(
        &self,
        media_path: &Path,
        duration: Option<Duration>,
    ) -> io::Result<(MediaIdentity, Option<LoadedRecord>)> {
        let mut identity = MediaIdentity::for_path(media_path, duration, false);
        let exact_name = record_name_for_path_key(&identity.path_key);
        if let Some(record) = self.read_record(&exact_name)? {
            if record.media_fingerprint.is_some() {
                identity.ensure_fingerprint();
            }
            if record.matches(&identity, true) {
                return Ok((
                    identity,
                    Some(LoadedRecord {
                        record_name: exact_name.clone(),
                        record,
                    }),
                ));
            }
        }

        identity.ensure_fingerprint();
        for candidate in self.rename_candidates(&exact_name)? {
            if let Some(record) = self.read_record(&candidate.name)?
                && record.matches(&identity, false)
            {
                return Ok((
                    identity,
                    Some(LoadedRecord {
                        record_name: candidate.name,
                        record,
                    }),
                ));
            }
        }

        Ok((identity, None))
    }

    pub(super) fn write_record(
        &self,
        record_name: &str,
        record: &ResumeRecord,
        durability: Durability,
    ) -> io::Result<()> {
        let text = record.to_text();
        if text.len() as u64 > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "playback state record exceeds the size limit",
            ));
        }
        self.create_dir()?;
        write_atomic(&self.record_path(record_name), text.as_bytes(), durability)
    }

    pub(super) fn read_record(&self, record_name: &str) -> io::Result<Option<ResumeRecord>> {
        let path = self.record_path(record_name);
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() > MAX_RECORD_BYTES {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Ok(None);
        }
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(None);
        };
        Ok(ResumeRecord::parse(&text))
    }

    pub(super) fn remove_record(&self, record_name: &str) -> io::Result<()> {
        match fs::remove_file(self.record_path(record_name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn create_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn record_files_newest_first(&self) -> io::Result<Vec<RecordFile>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut records = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry
                && let Some(record) = record_file(entry)
            {
                records.push(record);
            }
        }
        records.sort_by_key(|record| std::cmp::Reverse(record.modified));
        Ok(records)
    }

    pub(super) fn rename_candidates(&self, exact_name: &str) -> io::Result<Vec<RecordFile>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut newest = BinaryHeap::with_capacity(MAX_RENAME_CANDIDATES + 1);
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Some(record) = record_file(entry) else {
                continue;
            };
            if record.name == exact_name {
                continue;
            }
            newest.push(Reverse((record.modified, record.name)));
            if newest.len() > MAX_RENAME_CANDIDATES {
                newest.pop();
            }
        }

        let mut records = newest
            .into_iter()
            .map(|Reverse((modified, name))| RecordFile { name, modified })
            .collect::<Vec<_>>();
        records.sort_by_key(|record| Reverse(record.modified));
        Ok(records)
    }

    pub(super) fn housekeep(&self) -> io::Result<()> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let now = SystemTime::now();
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name == LEGACY_INDEX_FILE {
                remove_file_if_exists(&entry.path())?;
                continue;
            }
            if is_temp_name(name) {
                let stale = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age >= STALE_TEMP_AGE);
                if stale {
                    remove_file_if_exists(&entry.path())?;
                }
            }
        }
        self.remove_empty_dir()
    }

    pub(super) fn clear_all(&self) -> io::Result<usize> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.remove_empty_dir()?;
                return Ok(0);
            }
            Err(error) => return Err(error),
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if valid_record_name(&name) || is_temp_name(&name) || name == LEGACY_INDEX_FILE {
                remove_file_if_exists(&entry.path())?;
                removed += 1;
            }
        }
        self.sync_dir()?;
        self.remove_empty_dir()?;
        Ok(removed)
    }

    pub(super) fn sync_dir(&self) -> io::Result<()> {
        let dir = match fs::File::open(&self.dir) {
            Ok(dir) => dir,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        dir.sync_all()
    }

    pub(super) fn remove_empty_dir(&self) -> io::Result<()> {
        let remove_parent = match fs::remove_dir(&self.dir) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => false,
            Err(error) => return Err(error),
        };
        if remove_parent && let Some(parent) = self.dir.parent() {
            remove_dir_if_empty(parent)?;
        }
        Ok(())
    }

    pub(super) fn record_path(&self, record_name: &str) -> PathBuf {
        self.dir.join(record_name)
    }
}

pub(super) struct RecordFile {
    pub(super) name: String,
    modified: SystemTime,
}

pub(super) struct LoadedRecord {
    pub(super) record_name: String,
    pub(super) record: ResumeRecord,
}

fn record_file(entry: fs::DirEntry) -> Option<RecordFile> {
    let name = entry.file_name().to_str().map(str::to_string)?;
    if !valid_record_name(&name) {
        return None;
    }
    let metadata = entry.metadata().ok()?;
    metadata.is_file().then(|| RecordFile {
        name,
        modified: metadata.modified().unwrap_or(UNIX_EPOCH),
    })
}

pub(super) fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("resume");
    let serial = TMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!("{file_name}.tmp-{}-{serial}", std::process::id()))
}

pub(super) fn resume_state_home(
    xdg_state_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    xdg_state_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })
}

fn write_atomic(path: &Path, bytes: &[u8], durability: Durability) -> io::Result<()> {
    let tmp_path = temp_path_for(path);
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        if matches!(durability, Durability::Final) {
            file.sync_all()?;
        }
        drop(file);
        fs::rename(&tmp_path, path)?;
        if matches!(durability, Durability::Final)
            && let Some(parent) = path.parent()
        {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn valid_record_name(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_temp_name(name: &str) -> bool {
    let Some((record_name, suffix)) = name.split_once(".tmp-") else {
        return false;
    };
    if !valid_record_name(record_name) {
        return false;
    }
    let mut parts = suffix.split('-');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(pid), Some(serial), None)
            if !pid.is_empty()
                && pid.bytes().all(|byte| byte.is_ascii_digit())
                && !serial.is_empty()
                && serial.bytes().all(|byte| byte.is_ascii_digit())
    )
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_dir_if_empty(path: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}
