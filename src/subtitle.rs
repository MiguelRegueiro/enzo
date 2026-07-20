mod bitmap;

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(test)]
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use bitmap::draw_bitmap_subtitle;

use crate::{
    font::FontRenderer,
    font_system::{FontRole, FontSystem},
    media::{
        DecodedSubtitleBitmap, DecodedSubtitleCue, DecodedSubtitleTextKind, SubtitleStreamInfo,
        decode_subtitle_stream, load_subtitle_streams,
    },
    subtitle_language::{
        language_display_name, language_name, normalize_language_tag, subtitle_codec_label,
    },
};

const TEXT_COLOR: [u8; 3] = [255, 255, 255];
const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
const MAX_SUBTITLE_WIDTH_RATIO: f64 = 0.84;
const MAX_ACTIVE_SUBTITLE_LINES: usize = 3;
const LANGUAGE_DETECTION_SAMPLE_BYTES: usize = 16 * 1024;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubtitleCue {
    start: Duration,
    end: Duration,
    lines: Vec<String>,
    bitmap: Option<DecodedSubtitleBitmap>,
}

#[derive(Debug)]
pub(crate) struct SubtitleTrack {
    cues: Vec<SubtitleCue>,
    language: Option<String>,
    label: String,
}

impl SubtitleTrack {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = load_subtitle_text(path)?;
        let cues = parse_subtitle_text(path, &text)
            .with_context(|| format!("failed to parse subtitle file {}", path.display()))?;
        if cues.is_empty() {
            bail!("subtitle file has no cues: {}", path.display());
        }
        let language = infer_subtitle_language(path, &text);
        Ok(Self {
            label: external_subtitle_label(path, language.as_deref()),
            language,
            cues,
        })
    }

    pub(crate) fn with_label(mut self, label: String) -> Self {
        self.label = label;
        self
    }

    pub(crate) fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    fn active_lines(&self, position: Duration) -> Option<Vec<String>> {
        let mut lines = Vec::new();
        let end = self.cues.partition_point(|cue| cue.start <= position);
        for cue in &self.cues[..end] {
            if position >= cue.end {
                continue;
            }
            for line in &cue.lines {
                if !lines.contains(line) {
                    lines.push(line.clone());
                }
            }
        }
        if lines.len() > MAX_ACTIVE_SUBTITLE_LINES {
            lines.sort_by_key(|line| std::cmp::Reverse(line.chars().count()));
            lines.truncate(MAX_ACTIVE_SUBTITLE_LINES);
        }
        (!lines.is_empty()).then_some(lines)
    }

    fn active_bitmaps(&self, position: Duration) -> impl Iterator<Item = &DecodedSubtitleBitmap> {
        let end = self.cues.partition_point(|cue| cue.start <= position);
        self.cues[..end]
            .iter()
            .filter(move |cue| position < cue.end)
            .filter_map(|cue| cue.bitmap.as_ref())
    }
}

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
    Ok(Some(SubtitleTrack {
        cues,
        language,
        label: stream.label(),
    }))
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
        || !ass_dialogue_line_is_useful(fields.style, fields.effect, fields.text, text)
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

fn external_subtitle_label(path: &Path, language: Option<&str>) -> String {
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

fn load_subtitle_text(path: &Path) -> Result<String> {
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

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn matches_subtitle_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|expected| extension.eq_ignore_ascii_case(expected))
        })
}

pub(crate) struct SubtitleRenderer {
    font: Option<FontRenderer>,
    wrapped_lines: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct SubtitleLayout {
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) video_x: u32,
    pub(crate) video_y: u32,
    pub(crate) video_width: u32,
    pub(crate) video_height: u32,
}

impl SubtitleRenderer {
    pub(crate) fn new(fonts: &FontSystem, language: Option<&str>) -> Self {
        let fonts = fonts.resolve_all_for_language(FontRole::Subtitle, language);
        let font = open_first_font(&fonts, 26);
        Self {
            font,
            wrapped_lines: Vec::new(),
        }
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        layout: SubtitleLayout,
        track: &SubtitleTrack,
        position: Duration,
        bottom_reserve: u32,
    ) {
        let width = layout.canvas_width;
        let height = layout.canvas_height;
        if width == 0 || height == 0 || frame.len() < width as usize * height as usize * 3 {
            return;
        }
        for bitmap in track.active_bitmaps(position) {
            draw_bitmap_subtitle(frame, layout, bitmap, bottom_reserve);
        }
        let Some(lines) = track.active_lines(position) else {
            return;
        };

        let font_size = subtitle_font_size(width, height);
        let fallback_scale = fallback_text_scale(width, height);
        let mut font = if let Some(font) = self.font.as_mut() {
            font.set_pixel_size(font_size).then_some(font)
        } else {
            None
        };
        let line_height = font
            .as_ref()
            .map(|font| font.line_height())
            .unwrap_or(7 * fallback_scale)
            .max(1);
        let max_width = (f64::from(width) * MAX_SUBTITLE_WIDTH_RATIO).round() as u32;
        self.wrapped_lines.clear();
        wrap_subtitle_lines(
            &lines,
            max_width.max(1),
            fallback_scale,
            font.as_deref_mut(),
            &mut self.wrapped_lines,
        );
        if self.wrapped_lines.is_empty() {
            return;
        }

        let line_gap = (line_height / 5).max(2);
        let block_height = line_height
            .saturating_mul(self.wrapped_lines.len() as u32)
            .saturating_add(
                line_gap.saturating_mul(self.wrapped_lines.len().saturating_sub(1) as u32),
            );
        let bottom_margin = subtitle_bottom_margin(height)
            .max(bottom_reserve.saturating_add(8))
            .min(height.saturating_sub(1));
        let start_y = height
            .saturating_sub(bottom_margin)
            .saturating_sub(block_height);
        let mut y = start_y;

        for line in &self.wrapped_lines {
            let line_width = text_width(font.as_deref_mut(), line, fallback_scale);
            let x = width.saturating_sub(line_width) / 2;
            draw_subtitle_text(
                font.as_deref_mut(),
                frame,
                width,
                height,
                x,
                y,
                fallback_scale,
                line,
            );
            y = y.saturating_add(line_height).saturating_add(line_gap);
        }
    }
}

fn open_first_font(paths: &[PathBuf], pixel_size: u32) -> Option<FontRenderer> {
    paths
        .iter()
        .find_map(|path| FontRenderer::open_path(path, pixel_size))
}

fn parse_subtitle_text(path: &Path, text: &str) -> Result<Vec<SubtitleCue>> {
    if matches_subtitle_extension(path, &["ass", "ssa"]) || text.contains("[Events]") {
        parse_ass(text)
    } else if path_extension_is(path, "vtt")
        || text.trim_start_matches('\u{feff}').starts_with("WEBVTT")
    {
        parse_webvtt(text)
    } else {
        parse_srt(text)
    }
}

fn parse_ass(text: &str) -> Result<Vec<SubtitleCue>> {
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut in_events = false;
    let mut format = Vec::<String>::new();
    let mut cues = Vec::new();

    for line in normalized.lines().map(str::trim_end) {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[Events]") {
            in_events = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_events = false;
            continue;
        }
        if !in_events {
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("Format:") {
            format = value
                .split(',')
                .map(|field| field.trim().to_ascii_lowercase())
                .collect();
            continue;
        }

        let Some(value) = trimmed.strip_prefix("Dialogue:") else {
            continue;
        };
        if format.is_empty() {
            format = ass_default_format();
        }
        if let Some(cue) = parse_ass_dialogue(value.trim_start(), &format)? {
            cues.push(cue);
        }
    }

    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

fn ass_default_format() -> Vec<String> {
    [
        "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn decoded_ass_format() -> Vec<String> {
    [
        "readorder",
        "layer",
        "style",
        "name",
        "marginl",
        "marginr",
        "marginv",
        "effect",
        "text",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Clone, Copy)]
struct AssEventFields<'a> {
    style: &'a str,
    effect: &'a str,
    text: &'a str,
}

fn parse_ass_event_fields<'a>(line: &'a str, format: &[String]) -> Option<AssEventFields<'a>> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("Dialogue:")
        .map(str::trim_start)
        .unwrap_or(line);
    let fields = line.splitn(format.len(), ',').collect::<Vec<_>>();
    if fields.len() < format.len() {
        return None;
    }

    let field = |name: &str| -> Option<&'a str> {
        let index = format.iter().position(|field| field == name)?;
        fields.get(index).copied()
    };

    Some(AssEventFields {
        style: field("style").unwrap_or_default(),
        effect: field("effect").unwrap_or_default(),
        text: field("text")?,
    })
}

fn parse_ass_dialogue(line: &str, format: &[String]) -> Result<Option<SubtitleCue>> {
    let fields = line.splitn(format.len(), ',').collect::<Vec<_>>();
    if fields.len() < format.len() {
        return Ok(None);
    }

    let field = |name: &str| -> Option<&str> {
        let index = format.iter().position(|field| field == name)?;
        fields.get(index).copied()
    };

    let Some(start) = field("start") else {
        return Ok(None);
    };
    let Some(end) = field("end") else {
        return Ok(None);
    };
    let Some(text) = field("text") else {
        return Ok(None);
    };
    let style = field("style").unwrap_or_default().trim();
    let effect = field("effect").unwrap_or_default().trim();

    if is_ass_drawing(text) {
        return Ok(None);
    }

    let start = parse_timestamp(start.trim())?;
    let end = parse_timestamp(end.trim())?;
    if end <= start {
        return Ok(None);
    }

    let rendered_text = strip_srt_markup(text);
    let rendered_text = rendered_text.trim();
    if rendered_text.is_empty() || !ass_dialogue_line_is_useful(style, effect, text, rendered_text)
    {
        return Ok(None);
    }

    Ok(Some(SubtitleCue {
        start,
        end,
        lines: nonempty_subtitle_lines(rendered_text),
        bitmap: None,
    }))
}

fn is_ass_drawing(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        if matches!(chars.next(), Some('p' | 'P')) && matches!(chars.peek(), Some('1'..='9')) {
            return true;
        }
    }
    false
}

fn ass_dialogue_line_is_useful(style: &str, effect: &str, raw_text: &str, line: &str) -> bool {
    effect.trim().is_empty()
        && !ass_style_is_romanized_karaoke(style)
        && !ass_line_is_tiny_positioned_fragment(raw_text, line)
}

fn ass_style_is_romanized_karaoke(style: &str) -> bool {
    let normalized = style.trim().to_ascii_uppercase();
    normalized.starts_with("OP-R")
        || normalized.starts_with("ED-R")
        || normalized.contains("ROMAJI")
        || normalized.contains("ROMANJI")
}

fn ass_line_is_tiny_positioned_fragment(raw_text: &str, line: &str) -> bool {
    line.chars().count() <= 3 && (raw_text.contains("\\pos") || raw_text.contains("\\move"))
}

fn parse_srt(text: &str) -> Result<Vec<SubtitleCue>> {
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    let mut block = Vec::new();
    for line in normalized.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            parse_srt_block(&block, &mut cues)?;
            block.clear();
        } else {
            block.push(line);
        }
    }
    parse_srt_block(&block, &mut cues)?;
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

fn parse_webvtt(text: &str) -> Result<Vec<SubtitleCue>> {
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    let mut block = Vec::new();
    for line in normalized.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            parse_webvtt_block(&block, &mut cues)?;
            block.clear();
        } else {
            block.push(line);
        }
    }
    parse_webvtt_block(&block, &mut cues)?;
    cues.sort_by_key(|cue| cue.start);
    Ok(cues)
}

fn parse_webvtt_block(lines: &[&str], cues: &mut Vec<SubtitleCue>) -> Result<()> {
    let Some(first) = lines.first().map(|line| line.trim()) else {
        return Ok(());
    };
    if first.starts_with("WEBVTT")
        || first == "STYLE"
        || first == "REGION"
        || first == "NOTE"
        || first.starts_with("NOTE ")
    {
        return Ok(());
    }

    let Some(timing_index) = lines.iter().position(|line| line.contains("-->")) else {
        return Ok(());
    };
    let (start, end) = parse_timing_line(lines[timing_index])?;
    if end <= start {
        return Ok(());
    }
    let text_lines = lines[timing_index + 1..]
        .iter()
        .map(|line| strip_srt_markup(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if !text_lines.is_empty() {
        cues.push(SubtitleCue {
            start,
            end,
            lines: text_lines,
            bitmap: None,
        });
    }
    Ok(())
}

fn parse_srt_block(lines: &[&str], cues: &mut Vec<SubtitleCue>) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let timing_index = lines
        .iter()
        .position(|line| line.contains("-->"))
        .ok_or_else(|| anyhow!("subtitle block is missing timing line"))?;
    let (start, end) = parse_timing_line(lines[timing_index])?;
    if end <= start {
        return Ok(());
    }

    let text_lines = lines[timing_index + 1..]
        .iter()
        .map(|line| strip_srt_markup(line).trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if text_lines.is_empty() {
        return Ok(());
    }

    cues.push(SubtitleCue {
        start,
        end,
        lines: text_lines,
        bitmap: None,
    });
    Ok(())
}

fn parse_timing_line(line: &str) -> Result<(Duration, Duration)> {
    let mut parts = line.split("-->");
    let start = parts
        .next()
        .ok_or_else(|| anyhow!("subtitle timing is missing start"))?;
    let end = parts
        .next()
        .ok_or_else(|| anyhow!("subtitle timing is missing end"))?;
    if parts.next().is_some() {
        bail!("subtitle timing has too many separators");
    }

    let end = end.split_whitespace().next().unwrap_or(end);
    Ok((parse_timestamp(start.trim())?, parse_timestamp(end.trim())?))
}

fn parse_timestamp(text: &str) -> Result<Duration> {
    let text = text.replace(',', ".");
    let mut time_and_millis = text.split('.');
    let time = time_and_millis
        .next()
        .ok_or_else(|| anyhow!("subtitle timestamp is empty"))?;
    let millis = time_and_millis.next().unwrap_or("0");
    if time_and_millis.next().is_some() {
        bail!("subtitle timestamp has too many decimal separators");
    }

    let parts = time.split(':').collect::<Vec<_>>();
    if !matches!(parts.len(), 2 | 3) {
        bail!("subtitle timestamp must use MM:SS or HH:MM:SS format");
    }
    let (hours, minutes, seconds) = if parts.len() == 3 {
        (parts[0], parts[1], parts[2])
    } else {
        ("0", parts[0], parts[1])
    };
    let hours = hours.parse::<u64>().context("invalid subtitle hours")?;
    let minutes = minutes.parse::<u64>().context("invalid subtitle minutes")?;
    let seconds = seconds.parse::<u64>().context("invalid subtitle seconds")?;
    let millis = millis
        .chars()
        .take(3)
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect::<String>()
        .parse::<u64>()
        .context("invalid subtitle milliseconds")?;

    Ok(Duration::from_secs(
        hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(seconds),
    )
    .saturating_add(Duration::from_millis(millis)))
}

fn strip_srt_markup(line: &str) -> String {
    let without_ass = strip_ass_override_blocks(line);
    let without_html = strip_html_tags(&without_ass);
    decode_subtitle_entities(&normalize_ass_text_escapes(&without_html))
}

fn strip_ass_override_blocks(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };

        let block = &after_start[..end];
        if !looks_like_ass_override_block(block) {
            out.push('{');
            out.push_str(block);
            out.push('}');
        }
        rest = &after_start[end + 1..];
    }
    out.push_str(rest);
    out
}

fn looks_like_ass_override_block(block: &str) -> bool {
    let trimmed = block.trim();
    trimmed.starts_with('\\') || trimmed.contains('\\')
}

fn strip_html_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn normalize_ass_text_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('N') => {
                chars.next();
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Some('n' | 'h') => {
                chars.next();
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

fn nonempty_subtitle_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn decode_subtitle_entities(line: &str) -> String {
    line.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn infer_subtitle_language(path: &Path, text: &str) -> Option<String> {
    language_from_filename(path).or_else(|| detect_text_language(text))
}

fn language_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let normalized = stem.replace(['_', ' ', '[', ']', '(', ')'], ".");
    let parts = normalized
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    for pair in parts.windows(2) {
        let candidate = format!("{}-{}", pair[0], pair[1]);
        if let Some(language) = normalize_filename_language_tag(&candidate) {
            return Some(language);
        }
    }

    for part in &parts {
        if let Some(language) = normalize_filename_language_tag(part) {
            return Some(language);
        }
    }

    None
}

fn normalize_filename_language_tag(tag: &str) -> Option<String> {
    let language = normalize_language_tag(tag)?;
    language_name(&language).is_some().then_some(language)
}

fn detect_text_language(text: &str) -> Option<String> {
    let mut cjk = 0_u32;
    let mut kana = 0_u32;
    let mut hangul = 0_u32;
    let mut cyrillic = 0_u32;
    let mut latin = 0_u32;
    let mut english_stopwords = 0_u32;

    for line in text.lines().take(400) {
        let line = strip_srt_markup(line);
        if line.contains("-->") || line.trim().parse::<u32>().is_ok() {
            continue;
        }

        for ch in line.chars() {
            match ch {
                '\u{3040}'..='\u{30ff}' => kana += 1,
                '\u{3400}'..='\u{9fff}' => cjk += 1,
                '\u{ac00}'..='\u{d7af}' => hangul += 1,
                '\u{0400}'..='\u{04ff}' => cyrillic += 1,
                ch if ch.is_ascii_alphabetic() => latin += 1,
                _ => {}
            }
        }

        for word in line
            .split(|ch: char| !ch.is_ascii_alphabetic() && ch != '\'')
            .map(|word| word.to_ascii_lowercase())
        {
            if is_english_stopword(&word) {
                english_stopwords += 1;
            }
        }
    }

    if kana >= 4 {
        return Some("ja".to_string());
    }
    if hangul >= 4 {
        return Some("ko".to_string());
    }
    if cjk >= 4 {
        return Some("zh".to_string());
    }
    if cyrillic >= 12 {
        return Some("ru".to_string());
    }
    if latin >= 40 && english_stopwords >= 4 {
        return Some("en".to_string());
    }

    None
}

fn is_english_stopword(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "but"
            | "by"
            | "for"
            | "from"
            | "had"
            | "have"
            | "he"
            | "i"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "not"
            | "of"
            | "on"
            | "or"
            | "she"
            | "that"
            | "the"
            | "they"
            | "this"
            | "to"
            | "was"
            | "we"
            | "were"
            | "with"
            | "you"
            | "your"
    )
}

fn wrap_subtitle_lines(
    lines: &[String],
    max_width: u32,
    fallback_scale: u32,
    mut font: Option<&mut FontRenderer>,
    out: &mut Vec<String>,
) {
    for line in lines {
        let mut current = String::new();
        for word in line.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if text_width(font.as_deref_mut(), &candidate, fallback_scale) <= max_width
                || current.is_empty()
            {
                current = candidate;
            } else {
                out.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_subtitle_text(
    mut font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    fallback_scale: u32,
    text: &str,
) {
    for (dx, dy) in [
        (-2, 0),
        (2, 0),
        (0, -2),
        (0, 2),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ] {
        draw_text(
            font.as_deref_mut(),
            frame,
            width,
            height,
            x as i32 + dx,
            y as i32 + dy,
            fallback_scale,
            text,
            SHADOW_COLOR,
            230,
        );
    }
    draw_text(
        font,
        frame,
        width,
        height,
        x as i32,
        y as i32,
        fallback_scale,
        text,
        TEXT_COLOR,
        255,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    font: Option<&mut FontRenderer>,
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    fallback_scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    if let Some(font) = font {
        font.draw_text(frame, width, height, x, y, text, color, alpha);
    } else {
        draw_bitmap_text(
            frame,
            width,
            height,
            x.max(0) as u32,
            y.max(0) as u32,
            fallback_scale,
            text,
            color,
            alpha,
        );
    }
}

fn text_width(font: Option<&mut FontRenderer>, text: &str, fallback_scale: u32) -> u32 {
    font.map(|font| font.text_width(text))
        .unwrap_or_else(|| bitmap_text_width(text, fallback_scale))
}

fn subtitle_font_size(width: u32, height: u32) -> u32 {
    if width >= 960 && height >= 540 {
        34
    } else if width >= 420 && height >= 240 {
        26
    } else {
        16
    }
}

fn fallback_text_scale(width: u32, height: u32) -> u32 {
    if width >= 960 && height >= 540 {
        4
    } else if width >= 420 && height >= 240 {
        3
    } else {
        2
    }
}

fn subtitle_bottom_margin(height: u32) -> u32 {
    (height / 16).clamp(10, 46)
}

#[allow(clippy::too_many_arguments)]
fn draw_bitmap_text(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
    color: [u8; 3],
    alpha: u8,
) {
    let scale = scale.max(1);
    let mut cursor = x;
    for ch in text.chars() {
        if let Some(glyph) = glyph(ch) {
            draw_glyph(frame, width, height, cursor, y, scale, glyph, color, alpha);
        }
        cursor = cursor.saturating_add(6 * scale);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    glyph: [u8; 7],
    color: [u8; 3],
    alpha: u8,
) {
    for (row, bits) in glyph.into_iter().enumerate() {
        for col in 0..5_u32 {
            if bits & (1_u8 << (4 - col)) == 0 {
                continue;
            }
            fill_solid_rect(
                frame,
                width,
                height,
                x + col * scale,
                y + row as u32 * scale,
                scale,
                scale,
                color,
                alpha,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_solid_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    cols: u32,
    rows: u32,
    color: [u8; 3],
    alpha: u8,
) {
    for py in y..y.saturating_add(rows).min(height) {
        for px in x..x.saturating_add(cols).min(width) {
            let offset = rgb_offset(width, px, py);
            blend_pixel(frame, offset, color, alpha);
        }
    }
}

fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10011, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b11111, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b11111,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b11110, 0b00001, 0b00001, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b10010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01111, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b11110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '\'' => [
            0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        ' ' => [0; 7],
        _ => return None,
    })
}

fn bitmap_text_width(text: &str, scale: u32) -> u32 {
    let scale = scale.max(1);
    let chars = text.chars().count() as u32;
    if chars == 0 {
        0
    } else {
        chars * 6 * scale - scale
    }
}

fn blend_pixel(frame: &mut [u8], offset: usize, color: [u8; 3], alpha: u8) {
    let inverse = u16::from(255 - alpha);
    let alpha = u16::from(alpha);
    for channel in 0..3 {
        let source = u16::from(color[channel]) * alpha;
        let dest = u16::from(frame[offset + channel]) * inverse;
        frame[offset + channel] = ((source + dest + 127) / 255) as u8;
    }
}

fn rgb_offset(width: u32, x: u32, y: u32) -> usize {
    ((y * width + x) * 3) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_cues() {
        let cues = parse_srt(
            "\
1
00:00:01,500 --> 00:00:03,250
Hello
world
   
2
00:00:04.000 --> 00:00:05.000
<i>Bye</i>
",
        )
        .expect("srt should parse");

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Duration::from_millis(1500));
        assert_eq!(cues[0].end, Duration::from_millis(3250));
        assert_eq!(cues[0].lines, ["Hello", "world"]);
        assert_eq!(cues[1].lines, ["Bye"]);
    }

    #[test]
    fn parses_webvtt_without_external_conversion() {
        let cues = parse_webvtt(
            "\
WEBVTT - Enzo fixture

NOTE this block is ignored
not a cue

intro
00:01.500 --> 00:03.250 align:start position:10%
<i>Hello</i>
world

00:00:04.000 --> 00:00:05.000
Bye
",
        )
        .expect("webvtt should parse");

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Duration::from_millis(1500));
        assert_eq!(cues[0].end, Duration::from_millis(3250));
        assert_eq!(cues[0].lines, ["Hello", "world"]);
        assert_eq!(cues[1].lines, ["Bye"]);
    }

    #[test]
    fn decoded_ass_cues_keep_only_the_event_text() {
        let cue = subtitle_cue_from_decoded(DecodedSubtitleCue {
            start: Duration::from_secs(1),
            end: Duration::from_secs(2),
            kind: DecodedSubtitleTextKind::Ass,
            text: r"0,0,Default,,0,0,0,,{\an8}Hello\Nworld".to_string(),
            bitmap: None,
        })
        .expect("decoded cue should contain text");

        assert_eq!(cue.lines, ["Hello", "world"]);
    }

    #[test]
    fn language_detection_sample_is_bounded_on_utf8_boundaries() {
        let cues = vec![SubtitleCue {
            start: Duration::ZERO,
            end: Duration::from_secs(1),
            lines: vec!["字幕".repeat(LANGUAGE_DETECTION_SAMPLE_BYTES)],
            bitmap: None,
        }];

        let sample = subtitle_language_sample(&cues);

        assert!(sample.len() <= LANGUAGE_DETECTION_SAMPLE_BYTES);
        assert!(sample.is_char_boundary(sample.len()));
    }

    #[test]
    fn parses_ass_dialogues_and_overlapping_lines() {
        let cues = parse_ass(
            "\
[Script Info]
Title: test

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.56,0:00:05.27,OP-E1,,0,0,0,fx,{\\pos(960,1050)\\clip(1,2,3,4)}Surpass a fiction that nobody knows..
Dialogue: 0,0:00:00.60,0:00:01.60,OP-R1,,0,0,0,fx,{\\an5\\move(724.5,30,724.5,75,0,400)}m
Dialogue: 0,0:00:00.70,0:00:01.70,SeriesTitle,,0,0,0,,{\\p1}m 0 0 l 100 0 100 100 0 100
Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\an8}Normal line\\Nsecond half
",
        )
        .expect("ass should parse");

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, ["Normal line", "second half"]);

        let track = SubtitleTrack {
            cues,
            language: None,
            label: String::from("Subtitles"),
        };
        assert_eq!(
            track.active_lines(Duration::from_millis(1200)),
            Some(vec![
                String::from("Normal line"),
                String::from("second half")
            ])
        );
    }

    #[test]
    fn ass_song_fallback_skips_romaji_syllables_but_keeps_translation() {
        let cues = parse_ass(
            "\
[Script Info]
Title: test

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:22:39.29,0:22:44.88,ED-E,,0,0,0,,{\\pos(960,1050)\\3c&HAE641B&\\blur6}Not wanting to hide my eyes from sad happenings,
Dialogue: 0,0:22:39.35,0:22:40.00,ED-R1,,0,0,0,,{\\pos(439,54)}k
Dialogue: 0,0:22:39.38,0:22:40.03,ED-R1,,0,0,0,,{\\pos(467,54)}a
Dialogue: 0,0:22:39.41,0:22:40.06,ED-R1,,0,0,0,,{\\pos(495,54)}n
",
        )
        .expect("ass should parse");
        let track = SubtitleTrack {
            cues,
            language: None,
            label: String::from("Subtitles"),
        };

        assert_eq!(
            track.active_lines(Duration::from_millis(22 * 60 * 1000 + 40 * 1000)),
            Some(vec![String::from(
                "Not wanting to hide my eyes from sad happenings,"
            )])
        );
    }

    #[test]
    fn decoded_ass_song_fallback_skips_romaji_syllables_but_keeps_translation() {
        let translated = subtitle_cue_from_decoded(DecodedSubtitleCue {
            start: Duration::from_secs(1),
            end: Duration::from_secs(2),
            kind: DecodedSubtitleTextKind::Ass,
            text: r"0,0,ED-E,,0,0,0,,{\pos(960,1050)}Not wanting to hide my eyes".to_string(),
            bitmap: None,
        })
        .expect("translation should remain");
        let romaji = subtitle_cue_from_decoded(DecodedSubtitleCue {
            start: Duration::from_secs(1),
            end: Duration::from_secs(2),
            kind: DecodedSubtitleTextKind::Ass,
            text: r"1,0,ED-R1,,0,0,0,,{\pos(439,54)}k".to_string(),
            bitmap: None,
        });

        assert_eq!(translated.lines, ["Not wanting to hide my eyes"]);
        assert!(romaji.is_none());
    }

    #[test]
    fn strips_subtitle_markup_without_losing_literal_braces() {
        assert_eq!(strip_srt_markup(r"{\an8}{\i1}ku{\i0}"), "ku");
        assert_eq!(
            strip_srt_markup(r"hello {world} &amp; <i>friends</i>"),
            "hello {world} & friends"
        );
        assert_eq!(strip_srt_markup(r"one\Ntwo\hthree"), "one\ntwo three");
    }

    #[test]
    fn parses_srt_with_ass_override_tags_left_by_conversion() {
        let cues = parse_srt(
            "\
1
00:00:01,000 --> 00:00:02,000
{\\an8}ku

2
00:00:03,000 --> 00:00:04,000
{\\pos(10,20)}sign {literal}
",
        )
        .expect("srt should parse");

        assert_eq!(cues[0].lines, ["ku"]);
        assert_eq!(cues[1].lines, ["sign {literal}"]);
    }

    #[test]
    fn recognizes_text_and_bitmap_embedded_subtitle_codecs() {
        let ass = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
            subtitle_index: 0,
            codec: Some("ass".to_string()),
            language: Some("eng".to_string()),
            title: Some("English".to_string()),
            default: true,
            forced: false,
        });
        let pgs = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
            subtitle_index: 1,
            codec: Some("hdmv_pgs_subtitle".to_string()),
            language: Some("eng".to_string()),
            title: None,
            default: false,
            forced: false,
        });

        assert!(ass.is_supported());
        assert_eq!(ass.label(), "English [Default] [ASS]");
        assert!(pgs.is_supported());
    }

    #[test]
    fn embedded_subtitle_labels_use_title_with_language_and_codec_details() {
        let stream = |subtitle_index, language: &str, title: &str| {
            embedded_subtitle_stream_from_info(SubtitleStreamInfo {
                subtitle_index,
                codec: Some("ass".to_string()),
                language: Some(language.to_string()),
                title: Some(title.to_string()),
                default: false,
                forced: false,
            })
        };
        let cc = stream(1, "eng", "English(CC)");
        let portuguese = stream(2, "por", "Portuguese(Brazil)");
        let spanish = stream(3, "spa", "Spanish(Latin_America)");

        assert_eq!(cc.label(), "English [CC] [ASS]");
        assert_eq!(portuguese.label(), "Portuguese (Brazil) [ASS]");
        assert_eq!(spanish.label(), "Spanish (Latin America) [ASS]");
    }

    #[test]
    fn embedded_subtitle_labels_cover_common_untitled_stream_languages() {
        let cases = [
            ("ara", "Arabic [SRT]"),
            ("cze", "Czech [SRT]"),
            ("dan", "Danish [SRT]"),
            ("ger", "German [SRT]"),
            ("gre", "Greek [SRT]"),
            ("fin", "Finnish [SRT]"),
            ("fil", "Filipino [SRT]"),
            ("fre", "French [SRT]"),
            ("heb", "Hebrew [SRT]"),
            ("hrv", "Croatian [SRT]"),
            ("hun", "Hungarian [SRT]"),
            ("ind", "Indonesian [SRT]"),
            ("ita", "Italian [SRT]"),
            ("kor", "Korean [SRT]"),
            ("may", "Malay [SRT]"),
            ("nob", "Norwegian Bokmål [SRT]"),
            ("dut", "Dutch [SRT]"),
            ("pol", "Polish [SRT]"),
            ("por", "Portuguese [SRT]"),
            ("rum", "Romanian [SRT]"),
            ("swe", "Swedish [SRT]"),
            ("tha", "Thai [SRT]"),
            ("tur", "Turkish [SRT]"),
            ("ukr", "Ukrainian [SRT]"),
            ("vie", "Vietnamese [SRT]"),
            ("chi", "Chinese [SRT]"),
        ];

        for (language, expected) in cases {
            let stream = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
                subtitle_index: 0,
                codec: Some("subrip".to_string()),
                language: Some(language.to_string()),
                title: None,
                default: false,
                forced: false,
            });

            assert_eq!(stream.label(), expected, "language code {language}");
        }
    }

    #[test]
    fn embedded_subtitle_labels_preserve_unknown_language_tags() {
        let stream = embedded_subtitle_stream_from_info(SubtitleStreamInfo {
            subtitle_index: 0,
            codec: Some("subrip".to_string()),
            language: Some("ast".to_string()),
            title: None,
            default: false,
            forced: false,
        });

        assert_eq!(stream.label(), "ast [SRT]");
    }

    #[test]
    fn parses_short_millisecond_fields() {
        assert_eq!(
            parse_timestamp("00:00:01,5").expect("timestamp should parse"),
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_timestamp("00:00:01,05").expect("timestamp should parse"),
            Duration::from_millis(1050)
        );
    }

    #[test]
    fn active_lines_uses_current_position() {
        let track = SubtitleTrack {
            cues: parse_srt(
                "\
1
00:00:01,000 --> 00:00:02,000
One
",
            )
            .expect("srt should parse"),
            language: None,
            label: String::from("Subtitles"),
        };

        assert!(track.active_lines(Duration::from_millis(999)).is_none());
        assert_eq!(
            track.active_lines(Duration::from_millis(1000)),
            Some(vec![String::from("One")])
        );
        assert!(track.active_lines(Duration::from_millis(2000)).is_none());
    }

    #[test]
    fn active_lines_preserves_two_line_order() {
        let track = SubtitleTrack {
            cues: parse_srt(
                "\
1
00:00:01,000 --> 00:00:02,000
First line
Second line
",
            )
            .expect("srt should parse"),
            language: None,
            label: String::from("Subtitles"),
        };

        assert_eq!(
            track.active_lines(Duration::from_millis(1000)),
            Some(vec![
                String::from("First line"),
                String::from("Second line"),
            ])
        );
    }

    #[test]
    fn active_lines_caps_dense_ass_fallbacks_to_longest_lines() {
        let track = SubtitleTrack {
            cues: vec![
                SubtitleCue {
                    start: Duration::ZERO,
                    end: Duration::from_secs(1),
                    lines: vec![String::from("A")],
                    bitmap: None,
                },
                SubtitleCue {
                    start: Duration::ZERO,
                    end: Duration::from_secs(1),
                    lines: vec![String::from("long translated sentence")],
                    bitmap: None,
                },
                SubtitleCue {
                    start: Duration::ZERO,
                    end: Duration::from_secs(1),
                    lines: vec![String::from("medium label")],
                    bitmap: None,
                },
                SubtitleCue {
                    start: Duration::ZERO,
                    end: Duration::from_secs(1),
                    lines: vec![String::from("another useful line")],
                    bitmap: None,
                },
            ],
            language: None,
            label: String::from("Subtitles"),
        };

        assert_eq!(
            track.active_lines(Duration::ZERO),
            Some(vec![
                String::from("long translated sentence"),
                String::from("another useful line"),
                String::from("medium label"),
            ])
        );
    }

    #[test]
    fn sidecar_path_uses_srt_extension_for_local_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-subtitle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("movie.mp4");
        let subtitle = temp_dir.join("movie.srt");
        fs::write(&media, "").expect("media placeholder should be written");
        fs::write(&subtitle, "").expect("subtitle placeholder should be written");

        assert_eq!(sidecar_subtitle_paths(&media), [subtitle]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn sidecar_path_uses_supported_text_subtitle_extensions() {
        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-subtitle-extension-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("movie.mp4");
        let subtitle = temp_dir.join("movie.ass");
        fs::write(&media, "").expect("media placeholder should be written");
        fs::write(&subtitle, "").expect("subtitle placeholder should be written");

        assert_eq!(sidecar_subtitle_paths(&media), [subtitle]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn sidecar_paths_include_language_suffixed_siblings() {
        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-subtitle-siblings-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("movie.mkv");
        let simplified = temp_dir.join("movie.sc.ass");
        let traditional = temp_dir.join("movie.tc.ass");
        let unrelated = temp_dir.join("movie2.ass");
        for path in [&media, &traditional, &unrelated, &simplified] {
            fs::write(path, "").expect("fixture should be written");
        }

        assert_eq!(sidecar_subtitle_paths(&media), [simplified, traditional]);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn infers_language_from_sidecar_filename() {
        assert_eq!(
            language_from_filename(Path::new("movie.jpn.srt")),
            Some("ja".to_string())
        );
        assert_eq!(
            language_from_filename(Path::new("movie.zh.Hans.srt")),
            Some("zh-Hans".to_string())
        );
        assert_eq!(
            language_from_filename(Path::new("movie.sc.ass")),
            Some("zh-Hans".to_string())
        );
        assert_eq!(
            language_from_filename(Path::new("movie.tc.ass")),
            Some("zh-Hant".to_string())
        );
        assert_eq!(language_from_filename(Path::new("movie.srt")), None);
    }

    #[test]
    fn detects_english_subtitle_text_without_filename_language() {
        let text = "\
1
00:00:01,000 --> 00:00:03,000
They all said the tree was rotting.

2
00:00:04,000 --> 00:00:06,000
But I told them it was not.
";

        assert_eq!(detect_text_language(text), Some("en".to_string()));
    }

    #[test]
    fn detects_script_based_subtitle_languages() {
        assert_eq!(
            detect_text_language("Привет, как дела? Это тест."),
            Some("ru".to_string())
        );
        assert_eq!(
            detect_text_language("これは日本語の字幕です。"),
            Some("ja".to_string())
        );
        assert_eq!(
            detect_text_language("이것은 한국어 자막입니다."),
            Some("ko".to_string())
        );
        assert_eq!(
            detect_text_language("这是中文字幕。"),
            Some("zh".to_string())
        );
    }

    #[test]
    fn external_subtitle_label_keeps_source_and_detected_language() {
        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-external-subtitle-label-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let subtitle = temp_dir.join("movie.srt");
        fs::write(
            &subtitle,
            "1\n00:00:01,000 --> 00:00:03,000\nこれは日本語の字幕です。\n",
        )
        .expect("subtitle fixture should be written");

        let track = SubtitleTrack::load(&subtitle).expect("external subtitle should load");

        assert_eq!(track.language(), Some("ja"));
        assert_eq!(track.label(), "Japanese [External] [SRT]");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn loads_utf16le_external_subtitle_with_bom() {
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-utf16-subtitle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let subtitle = temp_dir.join("movie.srt");
        let text = "1\r\n00:00:01,000 --> 00:00:03,000\r\nWax on, wax off.\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&subtitle, bytes).expect("subtitle fixture should be written");

        let track = SubtitleTrack::load(&subtitle).expect("UTF-16 subtitle should load");

        assert_eq!(track.label(), "External [External] [SRT]");
        assert_eq!(
            track.active_lines(Duration::from_secs(1)),
            Some(vec![String::from("Wax on, wax off.")])
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn loads_embedded_srt_subtitle_when_ffmpeg_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-embedded-subtitle-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub = temp_dir.join("subtitle.srt");
        let media = temp_dir.join("embedded.mkv");
        let mut fixture = String::from(
            "1\n00:00:00,000 --> 00:00:01,000\nHello there, this is an embedded subtitle and you are in the test.\n\n",
        );
        for index in 1..130 {
            let start = index * 5;
            let end = start + 4;
            fixture.push_str(&format!(
                "{}\n00:00:00,{start:03} --> 00:00:00,{end:03}\nCue {index}\n\n",
                index + 1
            ));
        }
        fs::write(&sub, fixture).expect("subtitle fixture should be written");

        let status = Command::new("ffmpeg")
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("color=size=16x16:duration=1:rate=1")
            .arg("-f")
            .arg("srt")
            .arg("-i")
            .arg(&sub)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("1:s:0")
            .arg("-c:v")
            .arg("ffv1")
            .arg("-c:s")
            .arg("srt")
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            let _ = fs::remove_dir_all(&temp_dir);
            return;
        }

        let track = load_embedded_subtitle_tracks(&media)
            .expect("embedded subtitle load should not error")
            .into_iter()
            .next()
            .expect("embedded subtitle should be found");
        assert_eq!(track.cues.len(), 130);
        assert_eq!(track.language(), Some("en"));
        assert_eq!(
            track.active_lines(Duration::ZERO),
            Some(vec![String::from(
                "Hello there, this is an embedded subtitle and you are in the test.",
            )])
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn loads_embedded_ass_subtitle_without_srt_karaoke_flattening() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-embedded-ass-subtitle-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub = temp_dir.join("subtitle.ass");
        let media = temp_dir.join("embedded-ass.mkv");
        fs::write(
            &sub,
            "\
[Script Info]
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an8}Whole sentence, not syllable
",
        )
        .expect("subtitle fixture should be written");

        let status = Command::new("ffmpeg")
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("color=size=16x16:duration=1:rate=1")
            .arg("-i")
            .arg(&sub)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("1:s:0")
            .arg("-c:v")
            .arg("ffv1")
            .arg("-c:s")
            .arg("ass")
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            let _ = fs::remove_dir_all(&temp_dir);
            return;
        }

        let track = load_embedded_subtitle_tracks(&media)
            .expect("embedded subtitle load should not error")
            .into_iter()
            .next()
            .expect("embedded subtitle should be found");
        assert_eq!(
            track.active_lines(Duration::ZERO),
            Some(vec![String::from("Whole sentence, not syllable")])
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn renderer_draws_active_subtitle() {
        let track = SubtitleTrack {
            cues: parse_srt(
                "\
1
00:00:00,000 --> 00:00:10,000
Hello
",
            )
            .expect("srt should parse"),
            language: None,
            label: String::from("Subtitles"),
        };
        let mut renderer = SubtitleRenderer {
            font: None,
            wrapped_lines: Vec::new(),
        };
        let width = 320;
        let height = 180;
        let mut frame = vec![20_u8; (width * height * 3) as usize];

        renderer.render(
            &mut frame,
            SubtitleLayout {
                canvas_width: width,
                canvas_height: height,
                video_x: 0,
                video_y: 0,
                video_width: width,
                video_height: height,
            },
            &track,
            Duration::from_secs(1),
            0,
        );

        assert!(frame.iter().any(|&value| value > 220));
    }
}
