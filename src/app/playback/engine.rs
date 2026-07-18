use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::media::{AudioPlayer, FrameStatus, VideoDecoder};

use super::layout::TargetFrame;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AudioChoice {
    Off,
    Default,
    Stream(usize),
}

impl AudioChoice {
    pub(super) fn stream_index(self) -> Option<Option<usize>> {
        match self {
            Self::Off => None,
            Self::Default => Some(None),
            Self::Stream(index) => Some(Some(index)),
        }
    }
}

pub(super) struct PlaybackEngine {
    pub(super) video: VideoDecoder,
    pub(super) audio: Option<AudioPlayer>,
    pub(super) audio_done: bool,
    pub(super) video_ended: bool,
    pub(super) paused: bool,
    pub(super) muted: bool,
    pub(super) position: Duration,
    pub(super) frame_interval: Duration,
    pub(super) next_frame_at: Instant,
    pub(super) started_at: Instant,
}

impl PlaybackEngine {
    pub(super) fn open(
        path: &Path,
        target: TargetFrame,
        fps: f64,
        position: Duration,
        has_audio: bool,
        audio_choice: AudioChoice,
    ) -> Result<Self> {
        let video = VideoDecoder::spawn_at(path, target.width, target.height, fps, position, true)?;
        let audio = if has_audio {
            audio_choice
                .stream_index()
                .map(|stream_index| {
                    AudioPlayer::spawn_held_at(path, stream_index, position, false, false)
                })
                .transpose()?
        } else {
            None
        };
        video.set_audio_clock(audio.as_ref());
        let started_at = Instant::now();
        Ok(Self {
            video,
            audio,
            audio_done: !has_audio || audio_choice == AudioChoice::Off,
            video_ended: false,
            paused: false,
            muted: false,
            position,
            frame_interval: frame_interval(fps),
            next_frame_at: started_at,
            started_at,
        })
    }

    pub(super) fn poll_audio(&mut self) -> Result<()> {
        if let Some(player) = self.audio.as_mut()
            && player.is_finished()?
        {
            self.audio = None;
            self.audio_done = true;
        }
        Ok(())
    }

    pub(super) fn sync_audio_clock(&self) {
        self.video.set_audio_clock(self.audio.as_ref());
    }

    pub(super) fn toggle_pause(&mut self, seek_pending: bool) {
        self.paused = !self.paused;
        self.video.set_paused(self.paused || seek_pending);
        if let Some(audio) = self.audio.as_mut() {
            audio.set_paused(self.paused);
        }
        if !self.paused {
            self.next_frame_at = Instant::now();
        }
    }

    pub(super) fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(audio) = self.audio.as_mut() {
            audio.set_muted(self.muted);
        }
    }

    pub(super) fn restart_video(
        &mut self,
        path: &Path,
        target: TargetFrame,
        fps: f64,
        position: Duration,
    ) -> Result<()> {
        self.video.stop()?;
        self.video =
            VideoDecoder::spawn_at(path, target.width, target.height, fps, position, true)?;
        self.sync_audio_clock();
        self.position = position;
        self.video_ended = false;
        self.next_frame_at = Instant::now();
        Ok(())
    }

    pub(super) fn read_latest_frame(&mut self, frame: &mut [u8]) -> std::io::Result<FrameStatus> {
        self.video.read_latest_frame(frame)
    }

    pub(super) fn advance_frame_clock(&mut self) {
        self.next_frame_at += self.frame_interval;

        let now = Instant::now();
        if self.next_frame_at + self.frame_interval < now {
            self.next_frame_at = now + self.frame_interval;
        }
    }

    pub(super) fn complete(&self) -> bool {
        self.video_ended && self.audio_done
    }

    pub(super) fn stop(&mut self) -> Result<()> {
        self.video.request_stop();
        if let Some(audio) = self.audio.as_ref() {
            audio.request_stop();
        }
        let video_result = self.video.join();
        let audio_result = self.audio.as_mut().map(AudioPlayer::join).transpose();
        video_result?;
        audio_result?;
        Ok(())
    }
}

fn frame_interval(fps: f64) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1.0))
}
