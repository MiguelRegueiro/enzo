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

use super::{
    ffi::enzo_play_audio,
    native::{ErrorBuffer, duration_micros_i64, path_cstring},
};

#[derive(Clone)]
struct AudioThreadState {
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    mute: Arc<AtomicI32>,
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

    pub(super) fn playback_clock(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.shared.playback_micros)
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::media::ffi::{
        enzo_audio_seek_leading_silence_samples, enzo_audio_seek_trim_samples,
    };

    #[test]
    fn audio_seek_trimming_discards_early_frames_and_leading_samples() {
        let entirely_early = unsafe {
            enzo_audio_seek_trim_samples(1_000, 0, 1, 1_000, 1_024, 48_000, 1_030_000, 0, 1_024)
        };
        let crossing_target = unsafe {
            enzo_audio_seek_trim_samples(1_000, 0, 1, 1_000, 1_024, 48_000, 1_010_000, 17, 1_041)
        };
        let normalized_start = unsafe {
            enzo_audio_seek_trim_samples(
                11_400, 1_400, 1, 1_000, 1_024, 48_000, 10_005_000, 0, 1_024,
            )
        };
        let leading_silence =
            unsafe { enzo_audio_seek_leading_silence_samples(11_413, 1_400, 1, 1_000, 10_000_000) };
        let delayed_track_silence =
            unsafe { enzo_audio_seek_leading_silence_samples(500, 0, 1, 1_000, 0) };

        assert_eq!(entirely_early, -1);
        assert_eq!(crossing_target, 497);
        assert_eq!(normalized_start, 240);
        assert_eq!(leading_silence, 624);
        assert_eq!(delayed_track_silence, 24_000);
    }

    #[test]
    fn held_audio_seek_applies_and_prebuffers_before_release_when_pulse_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || !Command::new("pactl")
                .arg("info")
                .output()
                .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let media = std::env::temp_dir().join(format!(
            "enzo-held-audio-seek-test-{}.mkv",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("color=size=16x16:duration=2:rate=30")
            .args(["-f", "lavfi", "-i"])
            .arg("sine=frequency=440:sample_rate=48000:duration=2")
            .args([
                "-map", "0:v:0", "-map", "1:a:0", "-c:v", "ffv1", "-c:a", "flac",
            ])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let mut player =
            AudioPlayer::spawn_held_at(&media, None, Duration::from_millis(750), false, true)
                .expect("held audio player should start");
        let generation = player.seek_generation();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !player.seek_applied(generation) || !player.seek_buffered(generation) {
            assert!(
                !player.is_finished().expect("audio thread should not fail"),
                "held audio should not finish before release"
            );
            assert!(
                Instant::now() < deadline,
                "held audio should apply and buffer the seek"
            );
            thread::sleep(Duration::from_millis(2));
        }

        player.release_seek(generation);
        thread::sleep(Duration::from_millis(25));
        player.seek_held(Duration::from_millis(1_250));
        thread::sleep(Duration::from_millis(2));
        let stop_started = Instant::now();
        player.stop().expect("audio player should stop");
        assert!(
            stop_started.elapsed() < Duration::from_secs(1),
            "stopping during a held audio seek should be prompt"
        );

        let mut tail =
            AudioPlayer::spawn_held_at(&media, None, Duration::from_millis(1_990), false, true)
                .expect("held tail audio player should start");
        let tail_generation = tail.seek_generation();
        let tail_deadline = Instant::now() + Duration::from_secs(3);
        while !tail.seek_applied(tail_generation) || !tail.seek_buffered(tail_generation) {
            assert!(
                !tail
                    .is_finished()
                    .expect("tail audio thread should not fail"),
                "held tail audio should wait for release"
            );
            assert!(
                Instant::now() < tail_deadline,
                "held tail audio should apply and buffer the seek"
            );
            thread::sleep(Duration::from_millis(2));
        }
        tail.release_seek(tail_generation);
        while !tail
            .is_finished()
            .expect("tail audio thread should not fail")
        {
            assert!(
                Instant::now() < tail_deadline,
                "released tail audio should drain and finish"
            );
            thread::sleep(Duration::from_millis(2));
        }

        let _ = std::fs::remove_file(media);
    }
}
