use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::media::{AudioPlayer, VideoDecoder};

use super::engine::AudioChoice;

pub(super) struct SeekCoordinator {
    pub(super) pending: Option<PendingSeek>,
    pub(super) scrub_position: Option<Duration>,
    pub(super) keyboard_commit_at: Option<Instant>,
    pub(super) mouse_commit_at: Option<Instant>,
}

impl SeekCoordinator {
    pub(super) fn new(initial: PendingSeek) -> Self {
        Self {
            pending: Some(initial),
            scrub_position: None,
            keyboard_commit_at: None,
            mouse_commit_at: None,
        }
    }
}

pub(super) struct PendingSeek {
    pub(super) video_generation: i32,
    pub(super) video_target: Duration,
    pub(super) video_pts: Option<Duration>,
    pub(super) video_frame_displayed: bool,
    pub(super) audio_generation: Option<i32>,
    pub(super) audio_target: Option<Duration>,
    pub(super) release_requested: bool,
}

impl PendingSeek {
    pub(super) fn hold(&mut self) {
        self.release_requested = false;
    }

    pub(super) fn request_release(&mut self) {
        self.release_requested = true;
    }

    pub(super) fn needs_exact_retarget_for_release(&self, position: Duration) -> bool {
        self.video_target != position || !self.release_requested
    }

    pub(super) fn retarget_video(
        &mut self,
        decoder: &mut VideoDecoder,
        position: Duration,
        exact: bool,
    ) {
        self.video_generation = if exact {
            decoder.seek(position)
        } else {
            decoder.preview_seek(position)
        };
        self.video_target = position;
        self.video_pts = None;
        self.video_frame_displayed = false;
    }

    pub(super) fn mark_video_frame_displayed(&mut self, pts: Duration) {
        if self.video_pts.is_none() {
            self.video_pts = Some(pts);
        }
        if self.video_pts == Some(pts) {
            self.video_frame_displayed = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn seek_playback(
    path: &Path,
    has_audio: bool,
    decoder: &mut VideoDecoder,
    audio: &mut Option<AudioPlayer>,
    audio_done: &mut bool,
    audio_choice: AudioChoice,
    position: Duration,
    exact_video_seek: bool,
    paused: bool,
    muted: bool,
) -> Result<PendingSeek> {
    let video_generation = if exact_video_seek {
        decoder.seek(position)
    } else {
        decoder.preview_seek(position)
    };
    let mut audio_generation = None;
    if has_audio && let Some(audio_stream_index) = audio_choice.stream_index() {
        if let Some(audio) = audio.as_mut() {
            audio.set_paused(true);
            audio_generation = Some(audio.seek_held(position));
            audio.set_paused(paused);
            audio.set_muted(muted);
        } else {
            let player =
                AudioPlayer::spawn_held_at(path, audio_stream_index, position, paused, muted)?;
            audio_generation = Some(player.seek_generation());
            *audio = Some(player);
        }
        *audio_done = false;
    } else {
        if let Some(mut player) = audio.take() {
            player.stop()?;
        }
        *audio_done = true;
    }
    decoder.set_audio_clock(audio.as_ref());
    Ok(PendingSeek {
        video_generation,
        video_target: position,
        video_pts: None,
        video_frame_displayed: false,
        audio_generation,
        audio_target: audio_generation.map(|_| position),
        release_requested: true,
    })
}

pub(super) fn progress_pending_seek(
    pending: &mut Option<PendingSeek>,
    decoder: &VideoDecoder,
    audio: &mut Option<AudioPlayer>,
    paused: bool,
) -> bool {
    let Some(seek) = pending.as_mut() else {
        return false;
    };

    let Some(video_pts) = seek
        .video_pts
        .or_else(|| decoder.seek_frame(seek.video_generation))
    else {
        return false;
    };
    seek.video_pts = Some(video_pts);

    if !seek.release_requested {
        return false;
    }

    if seek.audio_generation.is_some()
        && let Some(player) = audio.as_ref()
        && seek.audio_target != Some(video_pts)
    {
        player.set_paused(true);
        seek.audio_generation = Some(player.seek_held(video_pts));
        seek.audio_target = Some(video_pts);
        player.set_paused(paused);
        return false;
    }

    let audio_ready = match (audio.as_ref(), seek.audio_generation) {
        (Some(player), Some(generation)) if paused => player.seek_applied(generation),
        (Some(player), Some(generation)) => player.seek_buffered(generation),
        _ => true,
    };
    if paused || !audio_ready {
        return false;
    }

    if let (Some(player), Some(generation)) = (audio.as_ref(), seek.audio_generation) {
        player.release_seek(generation);
    }
    decoder.release_seek(seek.video_generation, false);
    *pending = None;
    true
}

pub(super) fn mark_pending_seek_frame_displayed(pending: &mut Option<PendingSeek>, pts: Duration) {
    if let Some(seek) = pending.as_mut() {
        seek.mark_video_frame_displayed(pts);
    }
}

pub(super) fn keyboard_preview_target(
    pending: Option<&PendingSeek>,
    scrub_position: Option<Duration>,
    keyboard_seek_active: bool,
) -> Option<Duration> {
    let seek = pending?;
    let target = scrub_position?;
    (keyboard_seek_active
        && !seek.release_requested
        && seek.video_frame_displayed
        && seek.video_target != target)
        .then_some(target)
}

pub(super) fn advance_keyboard_seek_preview(
    pending: &mut Option<PendingSeek>,
    decoder: &mut VideoDecoder,
    scrub_position: Option<Duration>,
    keyboard_seek_active: bool,
) -> bool {
    let Some(target) =
        keyboard_preview_target(pending.as_ref(), scrub_position, keyboard_seek_active)
    else {
        return false;
    };
    let Some(seek) = pending.as_mut() else {
        return false;
    };
    seek.retarget_video(decoder, target, false);
    true
}

pub(super) fn resize_restart_position(
    playback_position: Duration,
    duration: Option<Duration>,
    paused: bool,
    audio: Option<&AudioPlayer>,
    pending_seek_target: Option<Duration>,
) -> Duration {
    let position = pending_seek_target.unwrap_or_else(|| {
        if paused {
            playback_position
        } else {
            audio
                .and_then(AudioPlayer::playback_position)
                .unwrap_or(playback_position)
        }
    });
    duration.map_or(position, |duration| position.min(duration))
}

pub(super) fn resize_pending_seek(
    video_generation: i32,
    video_target: Duration,
    interrupted_seek: Option<PendingSeek>,
) -> PendingSeek {
    let (audio_generation, audio_target) = interrupted_seek
        .map(|seek| (seek.audio_generation, seek.audio_target))
        .unwrap_or((None, None));
    PendingSeek {
        video_generation,
        video_target,
        video_pts: None,
        video_frame_displayed: false,
        audio_generation,
        audio_target,
        release_requested: true,
    }
}

pub(super) fn seek_from_progress_ratio(ratio: f64, duration: Option<Duration>) -> Option<Duration> {
    duration.map(|duration| Duration::from_secs_f64(duration.as_secs_f64() * ratio.clamp(0.0, 1.0)))
}

pub(super) fn seek_position(
    current: Duration,
    seconds: i32,
    duration: Option<Duration>,
) -> Duration {
    let delta = Duration::from_secs(seconds.unsigned_abs().into());
    let target = if seconds < 0 {
        current.saturating_sub(delta)
    } else {
        current.checked_add(delta).unwrap_or(Duration::MAX)
    };

    duration.map_or(target, |duration| target.min(duration))
}

pub(super) fn is_end_seek(target: Duration, duration: Option<Duration>) -> bool {
    duration.is_some_and(|duration| target >= duration)
}
