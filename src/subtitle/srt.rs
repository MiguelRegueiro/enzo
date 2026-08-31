use anyhow::{Result, anyhow};

use super::{
    parser::{parse_timing_line, strip_srt_markup},
    track::SubtitleCue,
};

#[cfg(test)]
#[path = "tests/srt.rs"]
mod tests;

pub(super) fn parse_srt(text: &str) -> Result<Vec<SubtitleCue>> {
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

pub(super) fn parse_srt_block(lines: &[&str], cues: &mut Vec<SubtitleCue>) -> Result<()> {
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
