use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicI32, AtomicI64, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};

use crate::decoder_backend::{
    backend_bindings::enzo_play_audio,
    backend_support::{ErrorBuffer, duration_micros_i64, path_cstring},
};

#[derive(Clone)]
struct AudioThreadState {
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    mute: Arc<AtomicI32>,
    volume_percent: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    released_seek_generation: Arc<AtomicI32>,
    applied_seek_generation: Arc<AtomicI32>,
    buffered_seek_generation: Arc<AtomicI32>,
    playback_micros: Arc<AtomicI64>,
}

pub(crate) struct AudioPlayer {
    shared: AudioThreadState,
    handle: Option<thread::JoinHandle<Result<()>>>,
    finished: bool,
}

impl AudioPlayer {
    pub(crate) fn spawn_held_at(
        path: &Path,
        audio_stream_index: Option<usize>,
        position: Duration,
        paused: bool,
        muted: bool,
        volume_percent: u16,
    ) -> Result<Self> {
        let path = path_cstring(path)?;
        let audio_stream_index = audio_stream_index
            .map(i32::try_from)
            .transpose()
            .context("audio stream index is too large")?
            .filter(|index| *index >= 0)
            .unwrap_or(-1);
        let initial_seek_generation = 1;
        let shared = AudioThreadState {
            stop: Arc::new(AtomicI32::new(0)),
            pause: Arc::new(AtomicI32::new(i32::from(paused))),
            mute: Arc::new(AtomicI32::new(i32::from(muted))),
            volume_percent: Arc::new(AtomicI32::new(i32::from(volume_percent))),
            seek_generation: Arc::new(AtomicI32::new(initial_seek_generation)),
            seek_micros: Arc::new(AtomicI64::new(duration_micros_i64(position))),
            released_seek_generation: Arc::new(AtomicI32::new(
                initial_seek_generation.wrapping_sub(1),
            )),
            applied_seek_generation: Arc::new(AtomicI32::new(0)),
            buffered_seek_generation: Arc::new(AtomicI32::new(0)),
            playback_micros: Arc::new(AtomicI64::new(-1)),
        };
        let thread_state = shared.clone();
        let handle = thread::spawn(move || {
            let mut error = ErrorBuffer::new();
            let status = unsafe {
                enzo_play_audio(
                    path.as_ptr(),
                    audio_stream_index,
                    thread_state.stop.as_ptr(),
                    thread_state.pause.as_ptr(),
                    thread_state.mute.as_ptr(),
                    thread_state.volume_percent.as_ptr(),
                    thread_state.seek_generation.as_ptr(),
                    thread_state.seek_micros.as_ptr(),
                    thread_state.released_seek_generation.as_ptr(),
                    thread_state.applied_seek_generation.as_ptr(),
                    thread_state.buffered_seek_generation.as_ptr(),
                    thread_state.playback_micros.as_ptr(),
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            if status < 0 {
                bail!("{}", error.message("audio playback failed"));
            }
            Ok(())
        });

        Ok(Self {
            shared,
            handle: Some(handle),
            finished: false,
        })
    }

    pub(crate) fn is_finished(&mut self) -> Result<bool> {
        if self.finished {
            return Ok(true);
        }
        let Some(handle) = self.handle.as_ref() else {
            self.finished = true;
            return Ok(true);
        };
        if !handle.is_finished() {
            return Ok(false);
        }

        let handle = self.handle.take().expect("audio handle should exist");
        self.finished = true;
        handle
            .join()
            .unwrap_or_else(|_| Err(anyhow!("audio playback thread panicked")))?;
        Ok(true)
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.request_stop();
        self.join()
    }

    pub(crate) fn request_stop(&self) {
        self.shared.stop.store(1, Ordering::Release);
    }

    pub(crate) fn join(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            self.finished = true;
            handle
                .join()
                .unwrap_or_else(|_| Err(anyhow!("audio playback thread panicked")))?;
        }
        self.finished = true;
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.shared
            .pause
            .store(i32::from(paused), Ordering::Release);
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.shared.mute.store(i32::from(muted), Ordering::Relaxed);
    }

    pub(crate) fn set_volume(&self, volume_percent: u16) {
        self.shared
            .volume_percent
            .store(i32::from(volume_percent), Ordering::Relaxed);
    }

    pub(crate) fn seek_held(&self, position: Duration) -> i32 {
        self.shared
            .seek_micros
            .store(duration_micros_i64(position), Ordering::Release);
        self.shared
            .seek_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    pub(crate) fn seek_generation(&self) -> i32 {
        self.shared.seek_generation.load(Ordering::Acquire)
    }

    pub(crate) fn seek_applied(&self, generation: i32) -> bool {
        self.shared.applied_seek_generation.load(Ordering::Acquire) == generation
    }

    pub(crate) fn seek_buffered(&self, generation: i32) -> bool {
        self.shared.buffered_seek_generation.load(Ordering::Acquire) == generation
    }

    pub(crate) fn release_seek(&self, generation: i32) {
        self.shared
            .released_seek_generation
            .store(generation, Ordering::Release);
    }

    pub(crate) fn playback_position(&self) -> Option<Duration> {
        let micros = self.shared.playback_micros.load(Ordering::Acquire);
        (micros >= 0).then(|| Duration::from_micros(micros as u64))
    }

    pub(crate) fn playback_clock(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.shared.playback_micros)
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
#[path = "tests/audio_player.rs"]
mod tests;
