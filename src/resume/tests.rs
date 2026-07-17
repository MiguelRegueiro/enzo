use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::{
    identity::{
        FINGERPRINT_CHUNK_BYTES, FileMetadata, MediaIdentity, duration_millis_close,
        file_fingerprint, path_key_for_media, record_name_for_path_key, system_time_millis,
    },
    model::{ResumeAudioSelection, ResumePlaybackState, ResumeSubtitleSelection},
    record::ResumeRecord,
    store::{
        Durability, LEGACY_INDEX_FILE, MAX_RENAME_CANDIDATES, ResumeStore, resume_state_home,
        temp_path_for,
    },
    tracker::{MINIMUM_RESUME_POSITION, ResumeTracker, SaveMode},
};

use std::ops::Deref;

#[test]
fn record_writes_compact_resume_state() {
    let media = PathBuf::from("/tmp/movie.mkv");
    let subtitle = PathBuf::from("/tmp/movie.en.srt");
    let identity = MediaIdentity {
        path_key: path_key_for_media(&media),
        metadata: Some(FileMetadata {
            len: 10,
            modified_ms: Some(20),
            dev: Some(30),
            ino: Some(40),
        }),
        duration: Some(Duration::from_secs(120)),
        fingerprint_path: None,
        fingerprint: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        ),
    };
    let state = ResumePlaybackState {
        position: Duration::from_millis(42_500),
        audio: ResumeAudioSelection::Selected {
            stream_index: Some(3),
            ordinal: Some(1),
            label: Some("English".to_string()),
        },
        subtitle: ResumeSubtitleSelection::external(
            &subtitle,
            &media,
            Some(0),
            Some("External".to_string()),
        ),
    };

    let record = ResumeRecord::from_state(&identity, state);
    let text = record.to_text();
    let parsed = ResumeRecord::parse(&text).expect("record should parse");

    assert!(text.contains("v=1\n"));
    assert!(text.contains("len=10\n"));
    assert!(text.contains("mtime=20\n"));
    assert!(text.contains("dur=120000\n"));
    assert!(text.contains("fpalg=sampled-sha256-v1\n"));
    assert!(text.contains("fp=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n"));
    assert!(text.contains("pos=42500\n"));
    assert!(text.contains("aid=3\n"));
    assert!(text.contains("aord=1\n"));
    assert!(text.contains("sub=external\n"));
    assert!(text.contains("sord=0\n"));
    assert!(!text.contains("media_path="));
    assert!(!text.contains("audio_label="));
    assert!(!text.contains("subtitle_label="));
    assert!(text.contains("alabel="));
    assert!(text.contains("slabel="));
    assert_eq!(parsed.position, Duration::from_millis(42_500));
    assert_eq!(
        parsed.audio,
        ResumeAudioSelection::Selected {
            stream_index: Some(3),
            ordinal: Some(1),
            label: Some("English".to_string()),
        }
    );
    assert!(matches!(
        parsed.subtitle,
        ResumeSubtitleSelection::External {
            ordinal: Some(0),
            label: Some(label),
            ..
        } if label == "External"
    ));
}

#[test]
fn restore_scans_records_after_rename() {
    let temp = test_dir("rename");
    let first = temp.join("first.mkv");
    let second = temp.join("second.mkv");
    fs::write(&first, b"video").expect("media should be written");

    let store = ResumeStore::new(temp.join("state"));
    let identity = MediaIdentity::for_path(&first, Some(Duration::from_secs(100)), true);
    let record_name = record_name_for_path_key(&identity.path_key);
    let record = ResumeRecord::from_state(&identity, playback_state(12));
    store
        .write_record(&record_name, &record, Durability::Checkpoint)
        .expect("record should write");

    fs::rename(&first, &second).expect("media should rename");
    let (_, loaded) = store
        .load(&second, Some(Duration::from_secs(100)))
        .expect("store should load");

    assert_eq!(
        loaded.expect("renamed record should load").record.position,
        Duration::from_secs(12)
    );
}

#[test]
fn saving_after_rename_retires_loaded_record() {
    let temp = test_dir("rename-save");
    let first = temp.join("first.mkv");
    let second = temp.join("second.mkv");
    fs::write(&first, b"video").expect("media should be written");
    let store = ResumeStore::new(temp.join("state"));
    let old_identity = MediaIdentity::for_path(&first, Some(Duration::from_secs(100)), true);
    let old_record_name = record_name_for_path_key(&old_identity.path_key);
    let old_record = ResumeRecord::from_state(
        &old_identity,
        ResumePlaybackState {
            position: Duration::from_secs(12),
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Off,
        },
    );
    store
        .write_record(&old_record_name, &old_record, Durability::Checkpoint)
        .expect("old record should write");
    fs::rename(&first, &second).expect("media should rename");

    let (identity, loaded) = store
        .load(&second, Some(Duration::from_secs(100)))
        .expect("renamed media should restore");
    let loaded = loaded.expect("old record should load after rename");
    let new_record_name = record_name_for_path_key(&identity.path_key);
    let mut tracker = ResumeTracker {
        store: Some(store.clone()),
        identity,
        record_name: new_record_name.clone(),
        loaded_record_name: Some(loaded.record_name),
        restored: None,
        state: ResumePlaybackState {
            position: Duration::from_secs(30),
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Off,
        },
        last_saved_state: None,
        last_checkpoint_at: Instant::now(),
        last_error: None,
        finished: false,
    };

    tracker
        .save_current(SaveMode::CleanupBelowThreshold, Durability::Checkpoint)
        .expect("save after rename should succeed");
    tracker.finished = true;

    assert!(
        store
            .read_record(&old_record_name)
            .expect("old record read should not fail")
            .is_none()
    );
    assert_eq!(
        store
            .read_record(&new_record_name)
            .expect("new record read should not fail")
            .expect("new record should exist")
            .position,
        Duration::from_secs(30)
    );
}

#[test]
fn exact_path_restore_rejects_replaced_file_with_different_size() {
    let media = PathBuf::from("/tmp/movie.mkv");
    let record_identity = MediaIdentity {
        path_key: path_key_for_media(&media),
        metadata: Some(FileMetadata {
            len: 10,
            modified_ms: None,
            dev: Some(1),
            ino: Some(2),
        }),
        duration: Some(Duration::from_secs(60)),
        fingerprint_path: None,
        fingerprint: Some(fingerprint(3)),
    };
    let opened_identity = MediaIdentity {
        path_key: path_key_for_media(&media),
        metadata: Some(FileMetadata {
            len: 99,
            modified_ms: None,
            dev: Some(1),
            ino: Some(2),
        }),
        duration: Some(Duration::from_secs(60)),
        fingerprint_path: None,
        fingerprint: Some(fingerprint(3)),
    };
    let record = ResumeRecord::from_state(
        &record_identity,
        ResumePlaybackState {
            position: Duration::from_secs(20),
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Off,
        },
    );

    assert!(!record.matches(&opened_identity, true));
}

#[test]
fn exact_path_restore_rejects_replaced_file_with_different_fingerprint() {
    let media = PathBuf::from("/tmp/movie.mkv");
    let record_identity = MediaIdentity {
        path_key: path_key_for_media(&media),
        metadata: Some(FileMetadata {
            len: 10,
            modified_ms: Some(20),
            dev: Some(1),
            ino: Some(2),
        }),
        duration: Some(Duration::from_secs(60)),
        fingerprint_path: None,
        fingerprint: Some(fingerprint(3)),
    };
    let opened_identity = MediaIdentity {
        path_key: path_key_for_media(&media),
        metadata: Some(FileMetadata {
            len: 10,
            modified_ms: Some(20),
            dev: Some(1),
            ino: Some(2),
        }),
        duration: Some(Duration::from_secs(60)),
        fingerprint_path: None,
        fingerprint: Some(fingerprint(4)),
    };
    let record = ResumeRecord::from_state(
        &record_identity,
        ResumePlaybackState {
            position: Duration::from_secs(20),
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Off,
        },
    );

    assert!(!record.matches(&opened_identity, true));
}

#[test]
fn duration_matching_tolerates_small_probe_drift() {
    assert!(duration_millis_close(60_000, 60_999));
    assert!(!duration_millis_close(60_000, 61_001));
}

#[test]
fn independent_records_coexist_without_shared_index_updates() {
    let temp = test_dir("coexist");
    let store = ResumeStore::new(temp.join("state"));
    let first = temp.join("first.mkv");
    let second = temp.join("second.mkv");
    fs::write(&first, b"first video").expect("first media should write");
    fs::write(&second, b"second video").expect("second media should write");
    write_state(&store, &first, 12);
    write_state(&store, &second, 34);

    let first_renamed = temp.join("first-renamed.mkv");
    let second_renamed = temp.join("second-renamed.mkv");
    fs::rename(&first, &first_renamed).expect("first media should rename");
    fs::rename(&second, &second_renamed).expect("second media should rename");

    assert_eq!(
        loaded_position(&store, &first_renamed),
        Duration::from_secs(12)
    );
    assert_eq!(
        loaded_position(&store, &second_renamed),
        Duration::from_secs(34)
    );
}

#[test]
fn below_threshold_save_does_not_create_empty_store() {
    let temp = test_dir("below-threshold-empty");
    let media = temp.join("movie.mkv");
    let store_dir = temp.join("state");
    fs::write(&media, b"video").expect("media should be written");

    let identity = MediaIdentity::for_path(&media, Some(Duration::from_secs(100)), false);
    let record_name = record_name_for_path_key(&identity.path_key);
    let mut tracker = ResumeTracker {
        store: Some(ResumeStore::new(store_dir.clone())),
        identity,
        record_name,
        loaded_record_name: None,
        restored: None,
        state: ResumePlaybackState::new(),
        last_saved_state: None,
        last_checkpoint_at: Instant::now(),
        last_error: None,
        finished: false,
    };

    tracker
        .save_current(SaveMode::CleanupBelowThreshold, Durability::Checkpoint)
        .expect("below-threshold save should not fail");
    tracker.finished = true;

    assert!(!store_dir.exists());
}

#[test]
fn below_threshold_save_removes_existing_record_and_empty_store() {
    let temp = test_dir("below-threshold-remove");
    let media = temp.join("movie.mkv");
    fs::write(&media, b"video").expect("media should be written");
    let store = ResumeStore::new(temp.join("state"));
    let identity = MediaIdentity::for_path(&media, Some(Duration::from_secs(100)), true);
    let record_name = record_name_for_path_key(&identity.path_key);
    let saved_state = ResumePlaybackState {
        position: Duration::from_secs(20),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::Off,
    };
    let record = ResumeRecord::from_state(&identity, saved_state);
    store
        .write_record(&record_name, &record, Durability::Checkpoint)
        .expect("record should write");

    let mut tracker = ResumeTracker {
        store: Some(store.clone()),
        identity,
        record_name: record_name.clone(),
        loaded_record_name: None,
        restored: None,
        state: ResumePlaybackState {
            position: MINIMUM_RESUME_POSITION - Duration::from_millis(1),
            audio: ResumeAudioSelection::Unspecified,
            subtitle: ResumeSubtitleSelection::Off,
        },
        last_saved_state: None,
        last_checkpoint_at: Instant::now(),
        last_error: None,
        finished: false,
    };

    tracker
        .save_current(SaveMode::CleanupBelowThreshold, Durability::Final)
        .expect("below-threshold save should remove stale resume");
    tracker.finished = true;

    assert!(
        store
            .read_record(&record_name)
            .expect("record read should not fail")
            .is_none()
    );
    assert!(!store.dir.exists());
}

#[test]
fn drop_before_playback_ready_preserves_existing_resume() {
    let temp = test_dir("startup-failure-preserve");
    let media = temp.join("movie.mkv");
    fs::write(&media, b"video").expect("media should be written");
    let store = ResumeStore::new(temp.join("state"));
    let identity = MediaIdentity::for_path(&media, Some(Duration::from_secs(100)), true);
    let record_name = record_name_for_path_key(&identity.path_key);
    let saved_state = ResumePlaybackState {
        position: Duration::from_secs(30),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::Off,
    };
    let record = ResumeRecord::from_state(&identity, saved_state);
    store
        .write_record(&record_name, &record, Durability::Checkpoint)
        .expect("record should write");

    let tracker = ResumeTracker {
        store: Some(store.clone()),
        identity,
        record_name: record_name.clone(),
        loaded_record_name: Some(record_name.clone()),
        restored: None,
        state: ResumePlaybackState::new(),
        last_saved_state: None,
        last_checkpoint_at: Instant::now(),
        last_error: None,
        finished: false,
    };

    drop(tracker);

    assert_eq!(
        store
            .read_record(&record_name)
            .expect("record read should not fail")
            .expect("record should be preserved")
            .position,
        Duration::from_secs(30)
    );
}

#[test]
fn temp_paths_are_unique_per_write_target() {
    let target = PathBuf::from("/tmp/enzo-resume-record");
    let first = temp_path_for(&target);
    let second = temp_path_for(&target);

    assert_ne!(first, second);
    assert_eq!(first.parent(), target.parent());
    assert!(
        first
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("enzo-resume-record.tmp-"))
    );
}

#[test]
fn fingerprint_detects_changes_in_the_middle_of_large_files() {
    let temp = test_dir("fingerprint-middle");
    let media = temp.join("movie.mkv");
    let mut bytes = vec![0_u8; (FINGERPRINT_CHUNK_BYTES * 4) as usize];
    fs::write(&media, &bytes).expect("media should write");
    let first = file_fingerprint(&media, bytes.len() as u64)
        .expect("fingerprint should read")
        .expect("fingerprint should exist");

    let middle = bytes.len() / 2;
    bytes[middle] = 1;
    fs::write(&media, &bytes).expect("changed media should write");
    let second = file_fingerprint(&media, bytes.len() as u64)
        .expect("fingerprint should read")
        .expect("fingerprint should exist");

    assert_ne!(first, second);
}

#[test]
fn housekeeping_removes_legacy_index_without_evicting_records() {
    let temp = test_dir("housekeep");
    let store = ResumeStore::new(temp.join("state"));
    store.create_dir().expect("store should be created");
    fs::write(store.dir.join(LEGACY_INDEX_FILE), "legacy").expect("legacy index should write");
    for index in 0..=MAX_RENAME_CANDIDATES {
        fs::write(store.dir.join(format!("{index:032x}")), "v=1\npos=5000\n")
            .expect("record should write");
    }

    store.housekeep().expect("housekeeping should succeed");

    assert!(!store.dir.join(LEGACY_INDEX_FILE).exists());
    assert_eq!(
        store
            .record_files_newest_first()
            .expect("records should list")
            .len(),
        MAX_RENAME_CANDIDATES + 1
    );
}

#[test]
fn clear_all_removes_only_resume_artifacts() {
    let temp = test_dir("clear-all");
    let store = ResumeStore::new(temp.join("state"));
    store.create_dir().expect("store should be created");
    let record_name = "0123456789abcdef0123456789abcdef";
    fs::write(store.dir.join(record_name), "v=1\npos=5000\n").expect("record should write");
    fs::write(
        store.dir.join(format!("{record_name}.tmp-1-0")),
        "temporary",
    )
    .expect("temporary file should write");
    fs::write(store.dir.join("keep-me"), "unknown").expect("unknown file should write");
    fs::write(store.dir.join("notes.tmp-backup"), "unknown")
        .expect("similarly named unknown file should write");

    assert_eq!(store.clear_all().expect("clear should succeed"), 2);
    assert!(store.dir.join("keep-me").exists());
    assert!(store.dir.join("notes.tmp-backup").exists());
}

#[test]
fn clear_all_removes_the_empty_application_state_directory() {
    let temp = test_dir("clear-empty-app-dir");
    let app_dir = temp.join("state/enzo");
    let store = ResumeStore::new(app_dir.join("watch_later"));
    store.create_dir().expect("store should be created");
    fs::write(
        store.dir.join("0123456789abcdef0123456789abcdef"),
        "v=1\npos=5000\n",
    )
    .expect("record should write");

    assert_eq!(store.clear_all().expect("clear should succeed"), 1);
    assert!(!store.dir.exists());
    assert!(!app_dir.exists());

    fs::create_dir_all(&app_dir).expect("empty app directory should be recreated");
    assert_eq!(store.clear_all().expect("second clear should succeed"), 0);
    assert!(!app_dir.exists());
}

#[test]
fn malformed_and_oversized_records_are_ignored() {
    let temp = test_dir("invalid-records");
    let store = ResumeStore::new(temp.join("state"));
    store.create_dir().expect("store should be created");
    let malformed_name = "0123456789abcdef0123456789abcdef";
    let oversized_name = "fedcba9876543210fedcba9876543210";
    fs::write(store.record_path(malformed_name), [0xff, 0xfe])
        .expect("malformed record should write");
    fs::write(
        store.record_path(oversized_name),
        vec![b'x'; super::store::MAX_RECORD_BYTES as usize + 1],
    )
    .expect("oversized record should write");

    assert!(
        store
            .read_record(malformed_name)
            .expect("malformed record should not fail the store")
            .is_none()
    );
    assert!(
        store
            .read_record(oversized_name)
            .expect("oversized record should not fail the store")
            .is_none()
    );
}

#[test]
fn rename_recovery_has_a_bounded_candidate_set_without_evicting_records() {
    let temp = test_dir("rename-scan-limit");
    let store = ResumeStore::new(temp.join("state"));
    let record = ResumeRecord {
        media_len: None,
        media_modified_ms: None,
        media_dev: None,
        media_ino: None,
        media_duration_ms: None,
        media_fingerprint: None,
        position: MINIMUM_RESUME_POSITION,
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::Off,
    };

    for index in 0..=MAX_RENAME_CANDIDATES {
        store
            .write_record(&format!("{index:032x}"), &record, Durability::Checkpoint)
            .expect("record should write");
    }

    assert_eq!(
        store
            .record_files_newest_first()
            .expect("records should list")
            .len(),
        MAX_RENAME_CANDIDATES + 1
    );
    assert_eq!(
        store
            .rename_candidates("ffffffffffffffffffffffffffffffff")
            .expect("rename candidates should list")
            .len(),
        MAX_RENAME_CANDIDATES
    );
}

#[test]
fn state_home_ignores_relative_base_directories() {
    assert_eq!(
        resume_state_home(
            Some(OsString::from("relative-state")),
            Some(OsString::from("/home/test")),
        ),
        Some(PathBuf::from("/home/test/.local/state"))
    );
    assert_eq!(
        resume_state_home(
            Some(OsString::from("relative-state")),
            Some(OsString::from("relative-home")),
        ),
        None
    );
}

#[cfg(unix)]
#[test]
fn store_and_records_use_private_permissions() {
    let temp = test_dir("permissions");
    let media = temp.join("movie.mkv");
    fs::write(&media, b"video").expect("media should write");
    let store = ResumeStore::new(temp.join("state"));
    write_state(&store, &media, 12);
    let identity = MediaIdentity::for_path(&media, Some(Duration::from_secs(100)), false);
    let record_name = record_name_for_path_key(&identity.path_key);

    assert_eq!(
        fs::metadata(&store.dir)
            .expect("store metadata should read")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(store.record_path(&record_name))
            .expect("record metadata should read")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn disabled_tracker_never_opens_a_store() {
    let tracker = ResumeTracker::open(
        Path::new("/tmp/disabled-resume.mkv"),
        Some(Duration::from_secs(10)),
        false,
    );

    assert!(tracker.store.is_none());
    assert!(tracker.restored().is_none());
}

#[test]
fn failed_final_save_remains_retryable() {
    let temp = test_dir("save-failure");
    let blocker = temp.join("not-a-directory");
    fs::write(&blocker, "file").expect("blocker should write");
    let media = temp.join("movie.mkv");
    fs::write(&media, b"video").expect("media should write");
    let identity = MediaIdentity::for_path(&media, Some(Duration::from_secs(100)), false);
    let mut tracker = ResumeTracker {
        store: Some(ResumeStore::new(blocker.join("state"))),
        record_name: record_name_for_path_key(&identity.path_key),
        identity,
        loaded_record_name: None,
        restored: None,
        state: playback_state(12),
        last_saved_state: None,
        last_checkpoint_at: Instant::now(),
        last_error: None,
        finished: false,
    };

    assert!(tracker.save_now().is_err());
    assert!(!tracker.finished);
    tracker.finished = true;
}

#[test]
fn external_subtitle_candidates_follow_media_directory() {
    let old_media = PathBuf::from("/old/movie.mkv");
    let subtitle = PathBuf::from("/old/subs/movie.srt");
    let new_media = PathBuf::from("/new/movie.mkv");
    let selection =
        ResumeSubtitleSelection::external(&subtitle, &old_media, Some(0), Some("Sub".into()));

    let candidates = selection.external_candidates(&new_media);

    assert!(candidates.contains(&subtitle));
    assert!(candidates.contains(&PathBuf::from("/new/subs/movie.srt")));
    assert!(candidates.contains(&PathBuf::from("/new/movie.srt")));
}

#[test]
fn external_subtitle_relative_path_uses_normalized_media_directory() {
    let temp = test_dir("subtitle-relative-path");
    let library = temp.join("library");
    let subtitle_dir = library.join("subs");
    fs::create_dir_all(&subtitle_dir).expect("subtitle directory should be created");
    let media = library.join("movie.mkv");
    let subtitle = subtitle_dir.join("english.srt");
    fs::write(&media, b"video").expect("media should be written");
    fs::write(&subtitle, b"subtitle").expect("subtitle should be written");
    let noncanonical_media = library.join("../library/movie.mkv");

    let selection =
        ResumeSubtitleSelection::external(&subtitle, &noncanonical_media, Some(0), None);

    assert!(matches!(
        selection,
        ResumeSubtitleSelection::External {
            relative_path: Some(ref path),
            ..
        } if path == Path::new("subs/english.srt")
    ));
}

fn fingerprint(value: u8) -> String {
    format!("{value:064x}")
}

fn playback_state(position_secs: u64) -> ResumePlaybackState {
    ResumePlaybackState {
        position: Duration::from_secs(position_secs),
        audio: ResumeAudioSelection::Unspecified,
        subtitle: ResumeSubtitleSelection::Off,
    }
}

fn write_state(store: &ResumeStore, media: &Path, position_secs: u64) {
    let identity = MediaIdentity::for_path(media, Some(Duration::from_secs(100)), true);
    let name = record_name_for_path_key(&identity.path_key);
    let record = ResumeRecord::from_state(&identity, playback_state(position_secs));
    store
        .write_record(&name, &record, Durability::Checkpoint)
        .expect("record should write");
}

fn loaded_position(store: &ResumeStore, media: &Path) -> Duration {
    store
        .load(media, Some(Duration::from_secs(100)))
        .expect("store should load")
        .1
        .expect("record should restore")
        .record
        .position
}

struct TestDir(PathBuf);

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_dir(name: &str) -> TestDir {
    let path = env::temp_dir().join(format!(
        "enzo-resume-{name}-{}-{}",
        std::process::id(),
        system_time_millis(SystemTime::now()).unwrap_or_default()
    ));
    fs::create_dir_all(&path).expect("test dir should be created");
    TestDir(path)
}
