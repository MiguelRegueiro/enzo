use std::{collections::BTreeSet, path::Path, time::Duration};

use anyhow::{Context, Result, bail};

use super::{
    embedded_decoder::DecodedSubtitleBitmap,
    language::infer_subtitle_language,
    parser::parse_subtitle_text,
    source::{external_subtitle_label, load_subtitle_text},
};

const MAX_ACTIVE_SUBTITLE_LINES: usize = 3;

#[cfg(test)]
#[path = "tests/track.rs"]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubtitleCue {
    pub(super) start: Duration,
    pub(super) end: Duration,
    pub(super) lines: Vec<String>,
    pub(super) bitmap: Option<DecodedSubtitleBitmap>,
}

#[derive(Debug)]
pub(crate) struct SubtitleTrack {
    pub(super) cues: Vec<SubtitleCue>,
    pub(super) text_timeline: Vec<SubtitleTextState>,
    language: Option<String>,
    label: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct SubtitleTextState {
    pub(super) start: Duration,
    pub(super) end: Duration,
    pub(super) lines: Vec<String>,
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
        let label = external_subtitle_label(path, language.as_deref());
        Ok(Self::from_cues(cues, language, label))
    }

    pub(super) fn from_cues(
        cues: Vec<SubtitleCue>,
        language: Option<String>,
        label: String,
    ) -> Self {
        let text_timeline = compile_text_timeline(&cues);
        Self {
            cues,
            text_timeline,
            language,
            label,
        }
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

    pub(super) fn active_lines(&self, position: Duration) -> Option<Vec<String>> {
        let end = self
            .text_timeline
            .partition_point(|state| state.start <= position);
        let state = end
            .checked_sub(1)
            .and_then(|index| self.text_timeline.get(index))?;
        (position < state.end).then(|| state.lines.clone())
    }

    pub(super) fn active_bitmaps(
        &self,
        position: Duration,
    ) -> impl Iterator<Item = &DecodedSubtitleBitmap> {
        let end = self.cues.partition_point(|cue| cue.start <= position);
        self.cues[..end]
            .iter()
            .filter(move |cue| position < cue.end)
            .filter_map(|cue| cue.bitmap.as_ref())
    }

    pub(super) fn upcoming_line_sets(&self, position: Duration, limit: usize) -> Vec<Vec<String>> {
        let start = self
            .text_timeline
            .partition_point(|state| state.start <= position);
        let mut upcoming = Vec::new();
        for state in &self.text_timeline[start..] {
            if !upcoming.contains(&state.lines) {
                upcoming.push(state.lines.clone());
            }
            if upcoming.len() >= limit {
                break;
            }
        }
        upcoming
    }
}

fn compile_text_timeline(cues: &[SubtitleCue]) -> Vec<SubtitleTextState> {
    let mut events = cues
        .iter()
        .enumerate()
        .filter(|(_, cue)| !cue.lines.is_empty() && cue.start < cue.end)
        .flat_map(|(index, cue)| [(cue.start, true, index), (cue.end, false, index)])
        .collect::<Vec<_>>();
    events.sort_unstable_by_key(|&(time, starts, index)| (time, starts, index));

    let mut timeline = Vec::<SubtitleTextState>::new();
    let mut active = BTreeSet::new();
    let mut cursor = 0;
    while cursor < events.len() {
        let time = events[cursor].0;
        while cursor < events.len() && events[cursor].0 == time {
            let (_, starts, index) = events[cursor];
            if starts {
                active.insert(index);
            } else {
                active.remove(&index);
            }
            cursor += 1;
        }
        let Some(next_time) = events.get(cursor).map(|event| event.0) else {
            break;
        };
        if active.is_empty() || time >= next_time {
            continue;
        }

        let mut lines = Vec::new();
        for &index in &active {
            for line in &cues[index].lines {
                if !lines.contains(line) {
                    lines.push(line.clone());
                }
            }
        }
        if lines.len() > MAX_ACTIVE_SUBTITLE_LINES {
            lines.sort_by_key(|line| std::cmp::Reverse(line.chars().count()));
            lines.truncate(MAX_ACTIVE_SUBTITLE_LINES);
        }
        if lines.is_empty() {
            continue;
        }
        if let Some(previous) = timeline.last_mut()
            && previous.end == time
            && previous.lines == lines
        {
            previous.end = next_time;
        } else {
            timeline.push(SubtitleTextState {
                start: time,
                end: next_time,
                lines,
            });
        }
    }
    timeline
}
