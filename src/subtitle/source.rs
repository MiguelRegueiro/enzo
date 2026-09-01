use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use super::{
    ass::{
        ass_default_format, ass_dialogue_line_is_useful, decoded_ass_format, is_ass_drawing,
        parse_ass_event_fields,
    },
    embedded_decoder::{DecodedSubtitleCue, DecodedSubtitleTextKind, decode_subtitle_stream},
    embedded_streams::{SubtitleStreamInfo, load_subtitle_streams},
    language::{
        detect_text_language, language_display_name, normalize_language_tag, subtitle_codec_label,
    },
    parser::{nonempty_subtitle_lines, strip_srt_markup},
    track::{SubtitleCue, SubtitleTrack},
};

const SUPPORTED_SUBTITLE_CODECS: &[&str] = &[
    "ass",
    "hdmv_pgs_subtitle",
    "ssa",
    "subrip",
    "srt",
    "text",
    "mov_text",
    "webvtt",
    "hdmv_text_subtitle",
];
pub(super) const LANGUAGE_DETECTION_SAMPLE_BYTES: usize = 16 * 1024;

#[cfg(test)]
#[path = "tests/source.rs"]
mod tests;

pub(crate) fn sidecar_subtitle_paths(media_path: &Path) -> Vec<PathBuf> {
    let text = media_path.as_os_str().to_string_lossy();
    if text.contains("://") {
        return Vec::new();
    }

    let Some(parent) = media_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = media_path.file_stem().map(|stem| stem.to_string_lossy()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let prefix = format!("{stem}.");
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with(&prefix))
                .unwrap_or(false)
                && matches_subtitle_extension(path, &["srt", "ass", "ssa", "vtt"])
        })
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| {
        (
            path.file_stem()
                .is_none_or(|subtitle_stem| subtitle_stem != stem.as_ref()),
            path.clone(),
        )
    });
    paths
}

#[cfg(test)]
pub(crate) fn load_embedded_subtitle_tracks(media_path: &Path) -> Result<Vec<SubtitleTrack>> {
    let streams = embedded_subtitle_streams(media_path);
    let mut tracks = Vec::new();
    for (fallback_index, stream) in streams.iter().enumerate() {
        if let Some(track) = load_embedded_subtitle_track(media_path, stream, fallback_index)? {
            tracks.push(track);
        }
    }
    Ok(tracks)
}

pub(crate) fn load_embedded_subtitle_track(
    media_path: &Path,
    stream: &EmbeddedSubtitleStream,
    fallback_index: usize,
) -> Result<Option<SubtitleTrack>> {
    if !stream.is_supported() {
        return Ok(None);
    }
    let subtitle_index = stream.subtitle_index.unwrap_or(fallback_index);
    let Ok(decoded) = decode_subtitle_stream(media_path, subtitle_index) else {
        return Ok(None);
    };
    let mut cues = decoded
        .into_iter()
        .filter_map(subtitle_cue_from_decoded)
        .collect::<Vec<_>>();
    if cues.is_empty() {
        return Ok(None);
    }
    cues.sort_by_key(|cue| cue.start);

    let sample = subtitle_language_sample(&cues);
    let language = stream
        .language
        .clone()
        .or_else(|| detect_text_language(&sample));
    Ok(Some(SubtitleTrack::from_cues(
        cues,
        language,
        stream.label(),
    )))
}

fn subtitle_language_sample(cues: &[SubtitleCue]) -> String {
    let mut sample = String::with_capacity(LANGUAGE_DETECTION_SAMPLE_BYTES);
    for line in cues.iter().flat_map(|cue| cue.lines.iter()) {
        if !sample.is_empty() && sample.len() < LANGUAGE_DETECTION_SAMPLE_BYTES {
            sample.push(' ');
        }
        for ch in line.chars() {
            if sample.len() + ch.len_utf8() > LANGUAGE_DETECTION_SAMPLE_BYTES {
                return sample;
            }
            sample.push(ch);
        }
    }
    sample
}

fn subtitle_cue_from_decoded(cue: DecodedSubtitleCue) -> Option<SubtitleCue> {
    if matches!(cue.kind, DecodedSubtitleTextKind::Bitmap) {
        return Some(SubtitleCue {
            start: cue.start,
            end: cue.end,
            lines: Vec::new(),
            bitmap: cue.bitmap,
        });
    }
    if matches!(cue.kind, DecodedSubtitleTextKind::Ass) {
        return subtitle_cue_from_decoded_ass(cue);
    }
    let text = cue.text.as_str();
    if is_ass_drawing(text) {
        return None;
    }
    let lines = text
        .lines()
        .map(|line| strip_srt_markup(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(SubtitleCue {
        start: cue.start,
        end: cue.end,
        lines,
        bitmap: None,
    })
}

fn subtitle_cue_from_decoded_ass(cue: DecodedSubtitleCue) -> Option<SubtitleCue> {
    let duration = cue.end.saturating_sub(cue.start);
    let decoded_format = decoded_ass_format();
    let dialogue_format = ass_default_format();
    let fields = if cue.text.trim_start().starts_with("Dialogue:") {
        parse_ass_event_fields(&cue.text, &dialogue_format)
    } else {
        parse_ass_event_fields(&cue.text, &decoded_format)
    };
    let Some(fields) = fields else {
        return subtitle_cue_from_decoded_plain_ass(cue);
    };
    if is_ass_drawing(fields.text) {
        return None;
    }
    let text = strip_srt_markup(fields.text);
    let text = text.trim();
    if text.is_empty()
        || !ass_dialogue_line_is_useful(fields.style, fields.effect, fields.text, text, duration)
    {
        return None;
    }
    Some(SubtitleCue {
        start: cue.start,
        end: cue.end,
        lines: nonempty_subtitle_lines(text),
        bitmap: None,
    })
}

fn subtitle_cue_from_decoded_plain_ass(cue: DecodedSubtitleCue) -> Option<SubtitleCue> {
    let text = decoded_ass_text(&cue.text);
    if is_ass_drawing(text) {
        return None;
    }
    let lines = text
        .lines()
        .map(|line| strip_srt_markup(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then_some(SubtitleCue {
        start: cue.start,
        end: cue.end,
        lines,
        bitmap: None,
    })
}

fn decoded_ass_text(event: &str) -> &str {
    let event = event.trim();
    if let Some(dialogue) = event.strip_prefix("Dialogue:").map(str::trim_start) {
        dialogue.splitn(10, ',').nth(9).unwrap_or(dialogue)
    } else {
        event.splitn(9, ',').nth(8).unwrap_or(event)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EmbeddedSubtitleStream {
    subtitle_index: Option<usize>,
    codec: Option<String>,
    language: Option<String>,
    title: Option<String>,
    default: bool,
    forced: bool,
}

impl EmbeddedSubtitleStream {
    pub(crate) fn label(&self) -> String {
        embedded_subtitle_label(self)
    }

    pub(crate) fn subtitle_index(&self) -> Option<usize> {
        self.subtitle_index
    }

    pub(crate) fn is_supported(&self) -> bool {
        self.codec
            .as_deref()
            .map(|codec| SUPPORTED_SUBTITLE_CODECS.contains(&codec))
            .unwrap_or(true)
    }
}

pub(crate) fn embedded_subtitle_streams(media_path: &Path) -> Vec<EmbeddedSubtitleStream> {
    load_subtitle_streams(media_path)
        .into_iter()
        .map(embedded_subtitle_stream_from_info)
        .collect()
}

fn embedded_subtitle_stream_from_info(info: SubtitleStreamInfo) -> EmbeddedSubtitleStream {
    EmbeddedSubtitleStream {
        subtitle_index: Some(info.subtitle_index),
        codec: info.codec,
        language: info.language.as_deref().and_then(normalize_language_tag),
        title: info.title,
        default: info.default,
        forced: info.forced,
    }
}

pub(super) fn external_subtitle_label(path: &Path, language: Option<&str>) -> String {
    let mut label = language
        .map(language_display_name)
        .unwrap_or_else(|| "External".to_string());
    label.push_str(" [External]");
    if let Some(codec) = path.extension().and_then(|extension| extension.to_str()) {
        label.push_str(" [");
        label.push_str(&subtitle_codec_label(codec));
        label.push(']');
    }
    label
}

fn embedded_subtitle_label(stream: &EmbeddedSubtitleStream) -> String {
    let language = stream.language.as_deref();
    let mut label = language
        .map(language_display_name)
        .unwrap_or_else(|| "Embedded".to_string());
    let mut flags = Vec::<String>::new();
    if let Some(title) = stream
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .and_then(|title| subtitle_title_label_part(language, title, &mut flags))
    {
        label.push_str(" (");
        label.push_str(&title);
        label.push(')');
    }
    if stream.default {
        push_unique_flag(&mut flags, "Default");
    }
    if stream.forced {
        push_unique_flag(&mut flags, "Forced");
    }
    for flag in flags {
        label.push_str(" [");
        label.push_str(&flag);
        label.push(']');
    }
    if let Some(codec) = stream.codec.as_deref().filter(|codec| !codec.is_empty()) {
        label.push_str(" [");
        label.push_str(&subtitle_codec_label(codec));
        label.push(']');
    }
    label
}

fn subtitle_title_label_part(
    language: Option<&str>,
    title: &str,
    flags: &mut Vec<String>,
) -> Option<String> {
    let normalized_title = title.replace('_', " ");
    let title = normalized_title.trim();
    if let Some(flag) = subtitle_title_flag(title) {
        push_unique_flag(flags, flag);
        return None;
    }
    let Some(language_name) = language.map(language_display_name) else {
        return Some(title.to_string());
    };
    if title.eq_ignore_ascii_case(&language_name) {
        return None;
    }
    let variant =
        title_language_qualifier(title, &language_name).unwrap_or_else(|| title.to_string());
    let variant = subtitle_variant_label(language, &variant);
    if let Some(flag) = subtitle_title_flag(&variant) {
        push_unique_flag(flags, flag);
        None
    } else {
        Some(variant)
    }
}

fn subtitle_title_flag(title: &str) -> Option<&'static str> {
    match title.trim().to_ascii_lowercase().as_str() {
        "cc" => Some("CC"),
        "sdh" | "sdh subtitles" | "hearing impaired" => Some("SDH"),
        "forced" | "forced narrative" => Some("Forced"),
        _ => None,
    }
}

fn push_unique_flag(flags: &mut Vec<String>, flag: &str) {
    if !flags.iter().any(|existing| existing == flag) {
        flags.push(flag.to_string());
    }
}

fn subtitle_variant_label(language: Option<&str>, variant: &str) -> String {
    match (language, variant.trim().to_ascii_lowercase().as_str()) {
        (Some("es"), "european" | "europe" | "spain") => "Spain".to_string(),
        (Some("pt"), "european" | "europe" | "portugal") => "Portugal".to_string(),
        (_, "latin american" | "latin america" | "latam") => "Latin America".to_string(),
        (_, "brazilian" | "brazil") => "Brazil".to_string(),
        (_, "simplified" | "simplified chinese") => "Simplified".to_string(),
        (_, "traditional" | "traditional chinese") => "Traditional".to_string(),
        _ => variant.trim().to_string(),
    }
}

fn title_language_qualifier(title: &str, language_name: &str) -> Option<String> {
    let rest = title.get(..language_name.len()).and_then(|prefix| {
        prefix
            .eq_ignore_ascii_case(language_name)
            .then(|| &title[language_name.len()..])
    })?;
    let qualifier = rest
        .trim_start_matches(|ch: char| {
            ch.is_ascii_whitespace() || matches!(ch, '-' | '_' | ':' | '(' | '[')
        })
        .trim_end_matches(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ')' | ']'));
    (!qualifier.is_empty()).then(|| qualifier.to_string())
}

pub(super) fn load_subtitle_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read subtitle file {}", path.display()))?;
    decode_subtitle_text(&bytes)
        .with_context(|| format!("failed to decode subtitle file {}", path.display()))
}

fn decode_subtitle_text(bytes: &[u8]) -> Result<String> {
    if let Some(bytes) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes.to_vec()).context("subtitle file is not valid UTF-8");
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16_subtitle(bytes, true);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16_subtitle(bytes, false);
    }
    String::from_utf8(bytes.to_vec()).context("subtitle file is not valid UTF-8")
}

fn decode_utf16_subtitle(bytes: &[u8], little_endian: bool) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        bail!("UTF-16 subtitle file has an odd byte count");
    }
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16(&units).context("subtitle file is not valid UTF-16")
}

pub(super) fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

pub(super) fn matches_subtitle_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|expected| extension.eq_ignore_ascii_case(expected))
        })
}
