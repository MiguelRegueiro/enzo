use std::{path::Path, time::Duration};

use anyhow::{Context, Result, anyhow, bail};

use super::{
    ass::parse_ass,
    source::{matches_subtitle_extension, path_extension_is},
    srt::parse_srt,
    track::SubtitleCue,
    webvtt::parse_webvtt,
};

#[cfg(test)]
#[path = "tests/parser.rs"]
mod tests;

pub(super) fn parse_subtitle_text(path: &Path, text: &str) -> Result<Vec<SubtitleCue>> {
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

pub(super) fn parse_timing_line(line: &str) -> Result<(Duration, Duration)> {
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

pub(super) fn parse_timestamp(text: &str) -> Result<Duration> {
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

pub(super) fn strip_srt_markup(line: &str) -> String {
    let without_ass = strip_ass_override_blocks(line);
    let without_html = strip_html_tags(&without_ass);
    decode_subtitle_entities(&normalize_ass_text_escapes(&without_html))
}

pub(super) fn strip_ass_override_blocks(line: &str) -> String {
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

pub(super) fn looks_like_ass_override_block(block: &str) -> bool {
    let trimmed = block.trim();
    trimmed.starts_with('\\') || trimmed.contains('\\')
}

pub(super) fn strip_html_tags(line: &str) -> String {
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

pub(super) fn normalize_ass_text_escapes(line: &str) -> String {
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

pub(super) fn nonempty_subtitle_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn decode_subtitle_entities(line: &str) -> String {
    line.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}
