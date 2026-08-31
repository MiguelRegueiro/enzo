use std::time::Duration;

use anyhow::Result;

use super::{
    parser::{nonempty_subtitle_lines, parse_timestamp, strip_srt_markup},
    track::SubtitleCue,
};

const MAX_TRANSIENT_POSITIONED_ASS_DURATION: Duration = Duration::from_millis(100);

#[cfg(test)]
#[path = "tests/ass.rs"]
mod tests;

pub(super) fn parse_ass(text: &str) -> Result<Vec<SubtitleCue>> {
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

pub(super) fn ass_default_format() -> Vec<String> {
    [
        "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(super) fn decoded_ass_format() -> Vec<String> {
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
pub(super) struct AssEventFields<'a> {
    pub(super) style: &'a str,
    pub(super) effect: &'a str,
    pub(super) text: &'a str,
}

pub(super) fn parse_ass_event_fields<'a>(
    line: &'a str,
    format: &[String],
) -> Option<AssEventFields<'a>> {
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

pub(super) fn parse_ass_dialogue(line: &str, format: &[String]) -> Result<Option<SubtitleCue>> {
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
    if rendered_text.is_empty()
        || !ass_dialogue_line_is_useful(style, effect, text, rendered_text, end - start)
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

pub(super) fn is_ass_drawing(text: &str) -> bool {
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

pub(super) fn ass_dialogue_line_is_useful(
    style: &str,
    effect: &str,
    raw_text: &str,
    line: &str,
    duration: Duration,
) -> bool {
    effect.trim().is_empty()
        && !ass_style_is_romanized_karaoke(style)
        && !ass_line_is_flattened_animation_fragment(raw_text, line, duration)
}

pub(super) fn ass_style_is_romanized_karaoke(style: &str) -> bool {
    let normalized = style.trim().to_ascii_uppercase();
    normalized.starts_with("OP-R")
        || normalized.starts_with("ED-R")
        || normalized.contains("ROMAJI")
        || normalized.contains("ROMANJI")
}

pub(super) fn ass_line_is_flattened_animation_fragment(
    raw_text: &str,
    line: &str,
    duration: Duration,
) -> bool {
    let is_positioned = raw_text.contains("\\pos") || raw_text.contains("\\move");
    is_positioned
        && (line.chars().count() <= 3 || duration <= MAX_TRANSIENT_POSITIONED_ASS_DURATION)
}
