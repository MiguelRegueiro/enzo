use std::{
    io::{self, ErrorKind},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{audio::AudioPlayer, decoder_backend::backend_support::duration_micros_i64};

use super::{
    video_frame_decoder::NativeVideoDecoder,
    video_frame_store::{LatestFrame, frame_len, frame_storage_len, validate_frame_guard},
    video_timing::DisplayRate,
    video_worker::{VideoThreadState, run_video_decode_thread},
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    NewFrame { pts: Duration },
    NoFrame,
    Ended,
}

pub(crate) struct VideoDecoder {
    shared: VideoThreadState,
    delivered_serial: u64,
    display_rate: DisplayRate,
    frame_thread: Option<thread::JoinHandle<()>>,
}

impl VideoDecoder {
    pub(crate) fn spawn_at(
        path: &Path,
        width: u32,
        height: u32,
        fps: f64,
        position: Duration,
        paused: bool,
    ) -> Result<Self> {
        let native = NativeVideoDecoder::open(path, width, height, fps)?;
        let frame_len = frame_len(width, height)?;
        let seek_generation = Arc::new(AtomicI32::new(i32::from(
            position > Duration::ZERO || paused,
        )));
        let initial_seek_generation = seek_generation.load(Ordering::Relaxed);
        let shared = VideoThreadState {
            latest_frame: Arc::new(Mutex::new(LatestFrame::with_reusable_buffer(frame_len)?)),
            stop: Arc::new(AtomicI32::new(0)),
            pause: Arc::new(AtomicI32::new(i32::from(paused))),
            seek_generation,
            seek_micros: Arc::new(AtomicI64::new(duration_micros_i64(position))),
            seek_exact: Arc::new(AtomicI32::new(1)),
            released_seek_generation: Arc::new(AtomicI32::new(initial_seek_generation)),
            master_clock: Arc::new(Mutex::new(None)),
        };
        let thread_state = shared.clone();

        let frame_thread = thread::spawn(move || {
            run_video_decode_thread(native, frame_len, fps, thread_state);
        });

        Ok(Self {
            shared,
            delivered_serial: 0,
            display_rate: DisplayRate::default(),
            frame_thread: Some(frame_thread),
        })
    }

    pub(crate) fn read_latest_frame(&mut self, frame: &mut [u8]) -> io::Result<FrameStatus> {
        let mut state = self
            .shared
            .latest_frame
            .lock()
            .map_err(|_| io::Error::other("video frame state is poisoned"))?;
        if state.serial != self.delivered_serial && state.ready {
            let Some(latest_frame) = state.frame.as_ref() else {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "video frame buffer is unavailable",
                ));
            };
            if latest_frame.len() != frame_storage_len(frame.len()).unwrap_or(usize::MAX) {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "video frame storage has {} bytes, expected {} plus its guard",
                        latest_frame.len(),
                        frame.len()
                    ),
                ));
            }
            validate_frame_guard(latest_frame, frame.len()).map_err(io::Error::other)?;
            frame.copy_from_slice(&latest_frame[..frame.len()]);
            self.delivered_serial = state.serial;
            let pts = state.pts;
            drop(state);
            self.display_rate.record(Instant::now());
            Ok(FrameStatus::NewFrame { pts })
        } else if let Some(error) = state.error.take() {
            Err(io::Error::other(error))
        } else if state.ended {
            Ok(FrameStatus::Ended)
        } else {
            Ok(FrameStatus::NoFrame)
        }
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.request_stop();
        self.join()
    }

    pub(crate) fn request_stop(&self) {
        self.shared.stop.store(1, Ordering::Release);
    }

    pub(crate) fn join(&mut self) -> Result<()> {
        if let Some(handle) = self.frame_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.shared
            .pause
            .store(i32::from(paused), Ordering::Relaxed);
    }

    pub(crate) fn display_fps(&self, now: Instant) -> Option<f64> {
        self.display_rate.measured_at(now)
    }

    pub(crate) fn seek(&mut self, position: Duration) -> i32 {
        self.seek_with_exactness(position, true)
    }

    pub(crate) fn preview_seek(&mut self, position: Duration) -> i32 {
        self.seek_with_exactness(position, false)
    }

    fn seek_with_exactness(&mut self, position: Duration, exact: bool) -> i32 {
        self.shared.pause.store(1, Ordering::Release);
        self.shared
            .seek_exact
            .store(i32::from(exact), Ordering::Release);
        self.shared
            .seek_micros
            .store(duration_micros_i64(position), Ordering::Release);
        let generation = self
            .shared
            .seek_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.display_rate.delivered_at.clear();
        if let Ok(mut state) = self.shared.latest_frame.lock() {
            state.ready = false;
            state.error = None;
            state.ended = false;
            state.serial = state.serial.wrapping_add(1);
            self.delivered_serial = state.serial;
        }
        generation
    }

    pub(crate) fn seek_frame(&self, generation: i32) -> Option<Duration> {
        let state = self.shared.latest_frame.lock().ok()?;
        (state.ready && state.seek_generation == generation).then_some(state.pts)
    }

    pub(crate) fn seek_generation(&self) -> i32 {
        self.shared.seek_generation.load(Ordering::Acquire)
    }

    pub(crate) fn release_seek(&self, generation: i32, paused: bool) {
        self.shared
            .released_seek_generation
            .store(generation, Ordering::Release);
        self.shared
            .pause
            .store(i32::from(paused), Ordering::Release);
    }

    pub(crate) fn set_audio_clock(&self, audio: Option<&AudioPlayer>) {
        if let Ok(mut master) = self.shared.master_clock.lock() {
            *master = audio.map(AudioPlayer::playback_clock);
        }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
#[path = "tests/video_decoder.rs"]
mod tests;
