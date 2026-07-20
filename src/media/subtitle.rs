use std::{
    ffi::{CStr, c_int},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail};

use super::{
    ffi::{
        EnzoDecodedSubtitleCue, EnzoDecodedSubtitleTrack, SUBTITLE_ASS, SUBTITLE_BITMAP,
        SUBTITLE_TEXT, enzo_decode_subtitle_stream, enzo_decoded_subtitle_track_free,
    },
    native::{ErrorBuffer, path_cstring},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DecodedSubtitleTextKind {
    Plain,
    Ass,
    Bitmap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedSubtitleBitmap {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) indices: Vec<u8>,
    pub(crate) palette_rgba: Box<[u8; 256 * 4]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedSubtitleCue {
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    pub(crate) kind: DecodedSubtitleTextKind,
    pub(crate) text: String,
    pub(crate) bitmap: Option<DecodedSubtitleBitmap>,
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
            canvas_width: 0,
            canvas_height: 0,
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
                SUBTITLE_BITMAP => DecodedSubtitleTextKind::Bitmap,
                _ => return None,
            };
            let start = u64::try_from(cue.start_micros).ok()?;
            let end = u64::try_from(cue.end_micros).ok()?;
            let bitmap = if matches!(kind, DecodedSubtitleTextKind::Bitmap) {
                Some(decoded_bitmap(
                    cue,
                    track.track.canvas_width,
                    track.track.canvas_height,
                )?)
            } else {
                None
            };
            let text = if matches!(kind, DecodedSubtitleTextKind::Bitmap) {
                String::new()
            } else {
                (!cue.text.is_null())
                    .then(|| unsafe { CStr::from_ptr(cue.text).to_string_lossy().into_owned() })?
            };
            (end > start).then_some(DecodedSubtitleCue {
                start: Duration::from_micros(start),
                end: Duration::from_micros(end),
                kind,
                text,
                bitmap,
            })
        })
        .collect();
    Ok(cues)
}

fn decoded_bitmap(
    cue: &EnzoDecodedSubtitleCue,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<DecodedSubtitleBitmap> {
    let len = (cue.bitmap_width as usize).checked_mul(cue.bitmap_height as usize)?;
    if len == 0
        || cue.bitmap_indices.is_null()
        || canvas_width == 0
        || canvas_height == 0
        || cue.bitmap_x.checked_add(cue.bitmap_width)? > canvas_width
        || cue.bitmap_y.checked_add(cue.bitmap_height)? > canvas_height
    {
        return None;
    }
    let indices = unsafe { std::slice::from_raw_parts(cue.bitmap_indices, len) }.to_vec();
    Some(DecodedSubtitleBitmap {
        x: cue.bitmap_x,
        y: cue.bitmap_y,
        width: cue.bitmap_width,
        height: cue.bitmap_height,
        canvas_width,
        canvas_height,
        indices,
        palette_rgba: Box::new(cue.palette_rgba),
    })
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
