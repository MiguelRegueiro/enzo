use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    encoding::{hex_decode, hex_encode, path_from_bytes, path_to_bytes},
    identity::{
        FINGERPRINT_ALGORITHM, FINGERPRINT_HEX_LEN, MediaIdentity, duration_millis_close,
        duration_millis_u64, durations_compatible,
    },
    model::{ResumeAudioSelection, ResumePlaybackState, ResumeSubtitleSelection},
};

const RECORD_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub(super) struct ResumeRecord {
    pub(super) media_len: Option<u64>,
    pub(super) media_modified_ms: Option<u64>,
    pub(super) media_dev: Option<u64>,
    pub(super) media_ino: Option<u64>,
    pub(super) media_duration_ms: Option<u64>,
    pub(super) media_fingerprint: Option<String>,
    pub(super) position: Duration,
    pub(super) audio: ResumeAudioSelection,
    pub(super) subtitle: ResumeSubtitleSelection,
}

impl ResumeRecord {
    pub(super) fn from_state(identity: &MediaIdentity, state: ResumePlaybackState) -> Self {
        Self {
            media_len: identity.metadata.as_ref().map(|metadata| metadata.len),
            media_modified_ms: identity
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.modified_ms),
            media_dev: identity.metadata.as_ref().and_then(|metadata| metadata.dev),
            media_ino: identity.metadata.as_ref().and_then(|metadata| metadata.ino),
            media_duration_ms: identity.duration.map(duration_millis_u64),
            media_fingerprint: identity.fingerprint.clone(),
            position: state.position,
            audio: state.audio,
            subtitle: state.subtitle,
        }
    }

    pub(super) fn matches(&self, identity: &MediaIdentity, exact_record_name: bool) -> bool {
        if exact_record_name {
            return self.compatible_metadata(identity)
                && self
                    .media_fingerprint
                    .as_ref()
                    .is_none_or(|fingerprint| identity.fingerprint.as_ref() == Some(fingerprint));
        }
        if let (Some(record_dev), Some(record_ino), Some(metadata)) =
            (self.media_dev, self.media_ino, identity.metadata.as_ref())
            && metadata.dev == Some(record_dev)
            && metadata.ino == Some(record_ino)
            && self.compatible_metadata(identity)
        {
            return true;
        }
        if let (Some(record_len), Some(record_hash), Some(metadata), Some(hash)) = (
            self.media_len,
            self.media_fingerprint.as_ref(),
            identity.metadata.as_ref(),
            identity.fingerprint.as_ref(),
        ) {
            let duration_matches = durations_compatible(self.media_duration_ms, identity.duration);
            if metadata.len == record_len && hash == record_hash && duration_matches {
                return true;
            }
        }
        false
    }

    pub(super) fn to_text(&self) -> String {
        let mut fields = Vec::new();
        fields.push(("v", RECORD_FORMAT_VERSION.to_string()));
        push_u64_field(&mut fields, "len", self.media_len);
        push_u64_field(&mut fields, "mtime", self.media_modified_ms);
        push_u64_field(&mut fields, "dev", self.media_dev);
        push_u64_field(&mut fields, "ino", self.media_ino);
        push_u64_field(&mut fields, "dur", self.media_duration_ms);
        if let Some(fingerprint) = &self.media_fingerprint {
            fields.push(("fpalg", FINGERPRINT_ALGORITHM.to_string()));
            fields.push(("fp", fingerprint.clone()));
        }
        fields.push(("pos", duration_millis_u64(self.position).to_string()));
        self.audio.push_fields(&mut fields);
        self.subtitle.push_fields(&mut fields);

        let mut text = String::new();
        for (key, value) in fields {
            text.push_str(key);
            text.push('=');
            text.push_str(&value);
            text.push('\n');
        }
        text
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        let fields = parse_fields(text);
        if parse_u32_field(&fields, "v") != Some(RECORD_FORMAT_VERSION) {
            return None;
        }

        Some(Self {
            media_len: parse_u64_field(&fields, "len"),
            media_modified_ms: parse_u64_field(&fields, "mtime"),
            media_dev: parse_u64_field(&fields, "dev"),
            media_ino: parse_u64_field(&fields, "ino"),
            media_duration_ms: parse_u64_field(&fields, "dur"),
            media_fingerprint: parse_fingerprint(&fields),
            position: Duration::from_millis(parse_u64_field(&fields, "pos")?),
            audio: ResumeAudioSelection::parse(&fields),
            subtitle: ResumeSubtitleSelection::parse(&fields),
        })
    }

    fn compatible_metadata(&self, identity: &MediaIdentity) -> bool {
        if let (Some(record_len), Some(metadata)) = (self.media_len, identity.metadata.as_ref())
            && metadata.len != record_len
        {
            return false;
        }
        if let (Some(record_modified_ms), Some(metadata)) =
            (self.media_modified_ms, identity.metadata.as_ref())
            && metadata
                .modified_ms
                .is_some_and(|modified_ms| modified_ms != record_modified_ms)
        {
            return false;
        }
        if let (Some(record_duration_ms), Some(duration)) =
            (self.media_duration_ms, identity.duration)
            && !duration_millis_close(record_duration_ms, duration_millis_u64(duration))
        {
            return false;
        }
        true
    }
}

impl ResumeAudioSelection {
    fn push_fields(&self, fields: &mut Vec<(&'static str, String)>) {
        match self {
            Self::Unspecified => {}
            Self::Disabled => fields.push(("audio", "off".to_string())),
            Self::Selected {
                stream_index,
                ordinal,
                label,
            } => {
                push_usize_field(fields, "aid", *stream_index);
                push_usize_field(fields, "aord", *ordinal);
                push_string_field(fields, "alabel", label.as_deref());
            }
        }
    }

    fn parse(fields: &HashMap<String, String>) -> Self {
        match fields.get("audio").map(String::as_str) {
            Some("off") => Self::Disabled,
            Some("unspecified") => Self::Unspecified,
            Some("selected") | None
                if fields.contains_key("aid")
                    || fields.contains_key("aord")
                    || fields.contains_key("alabel") =>
            {
                Self::Selected {
                    stream_index: parse_usize_field(fields, "aid"),
                    ordinal: parse_usize_field(fields, "aord"),
                    label: parse_string_field(fields, "alabel"),
                }
            }
            _ => Self::Unspecified,
        }
    }
}

impl ResumeSubtitleSelection {
    fn push_fields(&self, fields: &mut Vec<(&'static str, String)>) {
        match self {
            Self::Unspecified => {}
            Self::Off => fields.push(("sub", "off".to_string())),
            Self::External {
                path,
                relative_path,
                file_name,
                ordinal,
                label,
            } => {
                fields.push(("sub", "external".to_string()));
                push_path_field(fields, "spath", Some(path));
                push_path_field(fields, "srel", relative_path.as_deref());
                push_path_field(fields, "sfile", file_name.as_deref());
                push_usize_field(fields, "sord", *ordinal);
                push_string_field(fields, "slabel", label.as_deref());
            }
            Self::Embedded {
                stream_index,
                ordinal,
                label,
            } => {
                fields.push(("sub", "embedded".to_string()));
                push_usize_field(fields, "sid", *stream_index);
                push_usize_field(fields, "sord", *ordinal);
                push_string_field(fields, "slabel", label.as_deref());
            }
        }
    }

    fn parse(fields: &HashMap<String, String>) -> Self {
        match fields.get("sub").map(String::as_str) {
            Some("off") => Self::Off,
            Some("external") => {
                let Some(path) = parse_path_field(fields, "spath") else {
                    return Self::Unspecified;
                };
                Self::External {
                    path,
                    relative_path: parse_path_field(fields, "srel"),
                    file_name: parse_path_field(fields, "sfile"),
                    ordinal: parse_usize_field(fields, "sord"),
                    label: parse_string_field(fields, "slabel"),
                }
            }
            Some("embedded") => Self::Embedded {
                stream_index: parse_usize_field(fields, "sid"),
                ordinal: parse_usize_field(fields, "sord"),
                label: parse_string_field(fields, "slabel"),
            },
            _ => Self::Unspecified,
        }
    }
}

fn parse_fields(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn push_u64_field(fields: &mut Vec<(&'static str, String)>, key: &'static str, value: Option<u64>) {
    if let Some(value) = value {
        fields.push((key, value.to_string()));
    }
}

fn push_usize_field(
    fields: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<usize>,
) {
    if let Some(value) = value {
        fields.push((key, value.to_string()));
    }
}

fn push_hex_field(
    fields: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<&[u8]>,
) {
    if let Some(value) = value {
        fields.push((key, hex_encode(value)));
    }
}

fn push_string_field(
    fields: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<&str>,
) {
    push_hex_field(fields, key, value.map(str::as_bytes));
}

fn push_path_field(
    fields: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<&Path>,
) {
    push_hex_field(fields, key, value.map(path_to_bytes).as_deref());
}

fn parse_u32_field(fields: &HashMap<String, String>, key: &str) -> Option<u32> {
    fields.get(key)?.parse().ok()
}

fn parse_u64_field(fields: &HashMap<String, String>, key: &str) -> Option<u64> {
    fields.get(key)?.parse().ok()
}

fn parse_fingerprint(fields: &HashMap<String, String>) -> Option<String> {
    if fields.get("fpalg")? != FINGERPRINT_ALGORITHM {
        return None;
    }
    let fingerprint = fields.get("fp")?;
    (fingerprint.len() == FINGERPRINT_HEX_LEN
        && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| fingerprint.to_ascii_lowercase())
}

fn parse_usize_field(fields: &HashMap<String, String>, key: &str) -> Option<usize> {
    fields.get(key)?.parse().ok()
}

fn parse_string_field(fields: &HashMap<String, String>, key: &str) -> Option<String> {
    String::from_utf8(hex_decode(fields.get(key)?)?).ok()
}

fn parse_path_field(fields: &HashMap<String, String>, key: &str) -> Option<PathBuf> {
    hex_decode(fields.get(key)?).map(path_from_bytes)
}
