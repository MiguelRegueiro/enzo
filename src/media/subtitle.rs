use std::{
    ffi::{CStr, c_int},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail};

use super::{
    ffi::{
        EnzoDecodedSubtitleCue, EnzoDecodedSubtitleTrack, SUBTITLE_ASS, SUBTITLE_TEXT,
        enzo_decode_subtitle_stream, enzo_decoded_subtitle_track_free,
    },
    native::{ErrorBuffer, path_cstring},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DecodedSubtitleTextKind {
    Plain,
    Ass,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedSubtitleCue {
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    pub(crate) kind: DecodedSubtitleTextKind,
    pub(crate) text: String,
}

pub(crate) fn decode_subtitle_stream(
    path: &Path,
    subtitle_index: usize,
) -> Result<Vec<DecodedSubtitleCue>> {
    let path = path_cstring(path)?;
    let subtitle_index =
        c_int::try_from(subtitle_index).context("subtitle stream index is too large")?;
    let mut track = NativeDecodedSubtitleTrack {
        track: EnzoDecodedSubtitleTrack {
            cues: std::ptr::null_mut(),
            count: 0,
            capacity: 0,
        },
    };
    let mut error = ErrorBuffer::new();
    let status = unsafe {
        enzo_decode_subtitle_stream(
            path.as_ptr(),
            subtitle_index,
            &mut track.track,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status < 0 {
        bail!("{}", error.message("failed to decode subtitle stream"));
    }

    let cues = track
        .as_slice()
        .iter()
        .filter_map(|cue| {
            let kind = match cue.text_kind {
                SUBTITLE_TEXT => DecodedSubtitleTextKind::Plain,
                SUBTITLE_ASS => DecodedSubtitleTextKind::Ass,
                _ => return None,
            };
            let text = (!cue.text.is_null())
                .then(|| unsafe { CStr::from_ptr(cue.text).to_string_lossy().into_owned() })?;
            let start = u64::try_from(cue.start_micros).ok()?;
            let end = u64::try_from(cue.end_micros).ok()?;
            (end > start).then_some(DecodedSubtitleCue {
                start: Duration::from_micros(start),
                end: Duration::from_micros(end),
                kind,
                text,
            })
        })
        .collect();
    Ok(cues)
}

struct NativeDecodedSubtitleTrack {
    track: EnzoDecodedSubtitleTrack,
}

impl NativeDecodedSubtitleTrack {
    fn as_slice(&self) -> &[EnzoDecodedSubtitleCue] {
        if self.track.cues.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.track.cues, self.track.count) }
        }
    }
}

impl Drop for NativeDecodedSubtitleTrack {
    fn drop(&mut self) {
        unsafe {
            enzo_decoded_subtitle_track_free(&mut self.track);
        }
    }
}
