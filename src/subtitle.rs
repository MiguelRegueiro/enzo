use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    font::FontRenderer,
    font_system::{FontRole, FontSystem},
};

const TEXT_COLOR: [u8; 3] = [255, 255, 255];
const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
const MAX_SUBTITLE_WIDTH_RATIO: f64 = 0.84;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubtitleCue {
    start: Duration,
    end: Duration,
    lines: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct SubtitleTrack {
    cues: Vec<SubtitleCue>,
    language: Option<String>,
    label: String,
}

impl SubtitleTrack {
    pub(crate) fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read subtitle file {}", path.display()))?;
        let cues = parse_srt(&text)
            .with_context(|| format!("failed to parse subtitle file {}", path.display()))?;
        if cues.is_empty() {
            bail!("subtitle file has no cues: {}", path.display());
        }
        let language = infer_subtitle_language(path, &text);
        Ok(Self {
            label: subtitle_label(language.as_deref(), None, "External"),
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

    fn active_lines(&self, position: Duration) -> Option<&[String]> {
        let index = self
            .cues
            .partition_point(|cue| cue.start <= position)
            .checked_sub(1)?;
        let cue = &self.cues[index];
        (position < cue.end).then_some(cue.lines.as_slice())
    }
}

pub(crate) fn sidecar_subtitle_path(media_path: &Path) -> Option<PathBuf> {
    let text = media_path.as_os_str().to_string_lossy();
    if text.contains("://") {
        return None;
    }

    let path = media_path.with_extension("srt");
    path.is_file().then_some(path)
}

pub(crate) fn load_embedded_subtitle_tracks(media_path: &Path) -> Result<Vec<SubtitleTrack>> {
    let text = media_path.as_os_str().to_string_lossy();
    if text.contains("://") {
        return Ok(Vec::new());
    }

    let streams = embedded_subtitle_streams(media_path);
    let mut tracks = Vec::new();
    for (fallback_index, stream) in streams.into_iter().enumerate() {
        let subtitle_index = stream.subtitle_index.unwrap_or(fallback_index);
        let path = embedded_subtitle_temp_path(subtitle_index);
        let output = Command::new("ffmpeg")
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(media_path)
            .arg("-map")
            .arg(format!("0:s:{subtitle_index}"))
            .arg("-c:s")
            .arg("srt")
            .arg(&path)
            .output();

        let Ok(output) = output else {
            continue;
        };
        if !output.status.success()
            || fs::metadata(&path).map_or(true, |metadata| metadata.len() == 0)
        {
            let _ = fs::remove_file(&path);
            continue;
        }

        if let Ok(track) = SubtitleTrack::load(&path) {
            tracks.push(track.with_label(stream.label()));
        }
        let _ = fs::remove_file(&path);
    }
    Ok(tracks)
}

#[derive(Clone, Debug, Default)]
struct EmbeddedSubtitleStream {
    subtitle_index: Option<usize>,
    language: Option<String>,
    title: Option<String>,
    default: bool,
    forced: bool,
}

impl EmbeddedSubtitleStream {
    fn label(&self) -> String {
        subtitle_label(self.language.as_deref(), self.title.as_deref(), "Embedded")
    }
}

fn embedded_subtitle_streams(media_path: &Path) -> Vec<EmbeddedSubtitleStream> {
    let Ok(output) = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("s")
        .arg("-show_entries")
        .arg("stream=index:stream_tags=language,title:stream_disposition=default,forced")
        .arg("-of")
        .arg("compact=p=0:nk=0")
        .arg(media_path)
        .output()
    else {
        return vec![EmbeddedSubtitleStream {
            subtitle_index: Some(0),
            ..EmbeddedSubtitleStream::default()
        }];
    };
    if !output.status.success() {
        return vec![EmbeddedSubtitleStream {
            subtitle_index: Some(0),
            ..EmbeddedSubtitleStream::default()
        }];
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let streams = text
        .lines()
        .enumerate()
        .map(|(subtitle_index, line)| parse_embedded_subtitle_stream(line, subtitle_index))
        .collect::<Vec<_>>();
    if streams.is_empty() {
        vec![EmbeddedSubtitleStream {
            subtitle_index: Some(0),
            ..EmbeddedSubtitleStream::default()
        }]
    } else {
        streams
    }
}

fn parse_embedded_subtitle_stream(line: &str, subtitle_index: usize) -> EmbeddedSubtitleStream {
    let mut stream = EmbeddedSubtitleStream {
        subtitle_index: Some(subtitle_index),
        ..EmbeddedSubtitleStream::default()
    };
    for part in line.split('|') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "tag:language" => stream.language = normalize_language_tag(value),
            "tag:title" => stream.title = Some(value.to_string()),
            "disposition:default" => stream.default = value == "1",
            "disposition:forced" => stream.forced = value == "1",
            _ => {}
        }
    }
    stream
}

fn subtitle_label(language: Option<&str>, title: Option<&str>, fallback: &str) -> String {
    let language = language.map(language_label).unwrap_or(fallback);
    match title.filter(|title| !title.trim().is_empty()) {
        Some(title) => format!("{language} — {title}"),
        None => language.to_string(),
    }
}

fn embedded_subtitle_temp_path(subtitle_index: usize) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!(
        "rigoberto-embedded-subtitle-{}-{subtitle_index}-{nonce}.srt",
        process::id()
    ))
}

pub(crate) struct SubtitleRenderer {
    font: Option<FontRenderer>,
    wrapped_lines: Vec<String>,
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
        width: u32,
        height: u32,
        track: &SubtitleTrack,
        position: Duration,
        bottom_reserve: u32,
    ) {
        if width == 0 || height == 0 || frame.len() < width as usize * height as usize * 3 {
            return;
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
            lines,
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

fn language_label(language: &str) -> &'static str {
    match language {
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "zh-Hans" => "Chinese Simplified",
        "zh-Hant" => "Chinese Traditional",
        "ru" => "Russian",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "pt" => "Portuguese",
        "it" => "Italian",
        "ar" => "Arabic",
        "hi" => "Hindi",
        "tr" => "Turkish",
        _ => "Subtitles",
    }
}

fn open_first_font(paths: &[PathBuf], pixel_size: u32) -> Option<FontRenderer> {
    paths
        .iter()
        .find_map(|path| FontRenderer::open_path(path, pixel_size))
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
    if parts.len() != 3 {
        bail!("subtitle timestamp must use HH:MM:SS format");
    }
    let hours = parts[0].parse::<u64>().context("invalid subtitle hours")?;
    let minutes = parts[1]
        .parse::<u64>()
        .context("invalid subtitle minutes")?;
    let seconds = parts[2]
        .parse::<u64>()
        .context("invalid subtitle seconds")?;
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
        if let Some(language) = normalize_language_tag(&candidate) {
            return Some(language);
        }
    }

    for part in &parts {
        if let Some(language) = normalize_language_tag(part) {
            return Some(language);
        }
    }

    None
}

fn normalize_language_tag(tag: &str) -> Option<String> {
    let tag = tag.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-');
    if tag.is_empty() {
        return None;
    }
    let lower = tag.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "en" | "eng" => "en".to_string(),
        "ja" | "jp" | "jpn" => "ja".to_string(),
        "ko" | "kor" => "ko".to_string(),
        "zh" | "chi" | "zho" | "cn" => "zh".to_string(),
        "zh-hans" | "chi-hans" | "zho-hans" => "zh-Hans".to_string(),
        "zh-hant" | "chi-hant" | "zho-hant" => "zh-Hant".to_string(),
        "ru" | "rus" => "ru".to_string(),
        "es" | "spa" => "es".to_string(),
        "fr" | "fre" | "fra" => "fr".to_string(),
        "de" | "ger" | "deu" => "de".to_string(),
        "it" | "ita" => "it".to_string(),
        "pt" | "por" => "pt".to_string(),
        _ => return None,
    };
    Some(normalized)
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
            Some([String::from("One")].as_slice())
        );
        assert!(track.active_lines(Duration::from_millis(2000)).is_none());
    }

    #[test]
    fn sidecar_path_uses_srt_extension_for_local_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("rigoberto-subtitle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("movie.mp4");
        let subtitle = temp_dir.join("movie.srt");
        fs::write(&media, "").expect("media placeholder should be written");
        fs::write(&subtitle, "").expect("subtitle placeholder should be written");

        assert_eq!(sidecar_subtitle_path(&media), Some(subtitle));

        let _ = fs::remove_dir_all(&temp_dir);
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
    fn loads_embedded_srt_subtitle_when_ffmpeg_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-embedded-subtitle-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub = temp_dir.join("subtitle.srt");
        let media = temp_dir.join("embedded.mkv");
        fs::write(
            &sub,
            "1\n00:00:00,000 --> 00:00:01,000\nHello there, this is an embedded subtitle and you are in the test.\n",
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
        assert_eq!(track.language(), Some("en"));
        assert_eq!(
            track.active_lines(Duration::ZERO),
            Some(
                [String::from(
                    "Hello there, this is an embedded subtitle and you are in the test.",
                )]
                .as_slice()
            )
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

        renderer.render(&mut frame, width, height, &track, Duration::from_secs(1), 0);

        assert!(frame.iter().any(|&value| value > 220));
    }
}
