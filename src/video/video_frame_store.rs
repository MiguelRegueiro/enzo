use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, anyhow, bail};

// RGB conversion crosses an unsafe native boundary. Keep a protected tail
// after the payload so an overrun is reported before it reaches allocator
// metadata.
const FRAME_GUARD_BYTES: usize = 4 * 1024;
const FRAME_GUARD_VALUE: u8 = 0xa5;

#[derive(Default)]
pub(super) struct LatestFrame {
    pub(super) frame: Option<Vec<u8>>,
    pub(super) ready: bool,
    pub(super) pts: Duration,
    pub(super) seek_generation: i32,
    pub(super) ended: bool,
    pub(super) error: Option<String>,
    pub(super) serial: u64,
}

impl LatestFrame {
    pub(super) fn with_reusable_buffer(frame_len: usize) -> Result<Self> {
        Ok(Self {
            frame: Some(new_frame_buffer(frame_len)?),
            ..Self::default()
        })
    }
}

pub(super) fn frame_len(width: u32, height: u32) -> Result<usize> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("frame dimensions are too large"))?;
    pixels
        .checked_mul(3)
        .map(|bytes| bytes as usize)
        .ok_or_else(|| anyhow!("frame buffer is too large"))
}

pub(super) fn frame_storage_len(frame_len: usize) -> Result<usize> {
    frame_len
        .checked_add(FRAME_GUARD_BYTES)
        .ok_or_else(|| anyhow!("video frame storage is too large"))
}

pub(super) fn new_frame_buffer(frame_len: usize) -> Result<Vec<u8>> {
    let storage_len = frame_storage_len(frame_len)?;
    let mut frame = vec![0_u8; storage_len];
    frame[frame_len..].fill(FRAME_GUARD_VALUE);
    Ok(frame)
}

pub(super) fn validate_frame_guard(frame: &[u8], frame_len: usize) -> Result<()> {
    let expected_len = frame_storage_len(frame_len)?;
    if frame.len() != expected_len {
        bail!(
            "video frame storage has {} bytes, expected {expected_len}",
            frame.len()
        );
    }
    if frame[frame_len..]
        .iter()
        .any(|byte| *byte != FRAME_GUARD_VALUE)
    {
        bail!("native video decoder wrote past the RGB frame boundary");
    }
    Ok(())
}

pub(super) fn store_latest_frame(
    state: &Arc<Mutex<LatestFrame>>,
    frame: Vec<u8>,
    pts: Duration,
    seek_generation: &AtomicI32,
    seen_seek_generation: i32,
) -> Vec<u8> {
    if seek_generation.load(Ordering::Relaxed) != seen_seek_generation {
        return frame;
    }

    let Ok(mut state) = state.lock() else {
        return frame;
    };
    if seek_generation.load(Ordering::Relaxed) != seen_seek_generation {
        return frame;
    }

    let Some(old_frame) = state.frame.take() else {
        state.error = Some("reusable video frame buffer is unavailable".to_string());
        state.ended = true;
        state.serial = state.serial.wrapping_add(1);
        return frame;
    };
    state.frame = Some(frame);
    state.ready = true;
    state.pts = pts;
    state.seek_generation = seen_seek_generation;
    state.ended = false;
    state.serial = state.serial.wrapping_add(1);
    old_frame
}

pub(super) fn reset_frame_state(state: &Arc<Mutex<LatestFrame>>) {
    if let Ok(mut state) = state.lock() {
        state.ready = false;
        state.error = None;
        state.ended = false;
        state.serial = state.serial.wrapping_add(1);
    }
}

pub(super) fn mark_ended(state: &Arc<Mutex<LatestFrame>>) {
    if let Ok(mut state) = state.lock() {
        state.ended = true;
    }
}

pub(super) fn mark_error(state: &Arc<Mutex<LatestFrame>>, error: String) {
    if let Ok(mut state) = state.lock() {
        state.error = Some(error);
        state.ended = true;
    }
}

#[cfg(test)]
#[path = "tests/video_frame_store.rs"]
mod tests;
