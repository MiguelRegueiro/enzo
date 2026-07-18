use std::{
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    font_system::FontSystem,
    media::{FrameStatus, VideoInfo},
    resume::ResumeTracker,
    shutdown,
    subtitle::{SubtitleRenderer, SubtitleTrack},
    terminal::clear_screen_and_images,
};

use super::super::terminal_input::read_input_events;
#[cfg(test)]
use super::metadata::{container_display_name, format_file_size};
#[cfg(test)]
use super::ui::MEDIA_INFO_VISIBLE_FOR;
#[cfg(test)]
use super::ui::{MediaInfoOverlay, media_info_fps_visible, overlay_state, overlay_visible};
use super::{
    engine::PlaybackEngine,
    interaction::InteractionContext,
    layout::{ResizeTracker, terminal_target_and_canvas},
    seek::{
        SeekCoordinator, advance_keyboard_seek_preview, mark_pending_seek_frame_displayed,
        progress_pending_seek, resize_pending_seek, resize_restart_position, seek_playback,
    },
    subtitles::SubtitleCatalog,
    tracks::AudioCatalog,
    ui::{PlaybackUi, media_info_display_fps},
    view::PlaybackView,
};

#[cfg(test)]
use super::{
    layout::{CanvasFrame, RESIZE_SETTLE_FOR, TargetFrame},
    pointer::canvas_position as mouse_canvas_position,
    seek::{
        PendingSeek, is_end_seek, keyboard_preview_target, seek_from_progress_ratio, seek_position,
    },
    view::copy_video_into_canvas,
};
#[cfg(test)]
use crate::overlay::MediaInfo;

pub(super) struct PlaybackSession<'fonts, W: Write> {
    font_system: &'fonts FontSystem,
    path: PathBuf,
    source: VideoInfo,
    resume: ResumeTracker,
    audio: AudioCatalog,
    subtitles: SubtitleCatalog,
    engine: PlaybackEngine,
    view: PlaybackView<W>,
    ui: PlaybackUi,
    seeking: SeekCoordinator,
    resize: ResizeTracker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlaybackOutcome {
    Quit,
    Completed,
    Interrupted,
}

impl PlaybackOutcome {
    fn clears_resume(self) -> bool {
        self == Self::Completed
    }
}

pub(super) struct PlaybackSessionInit<'fonts, W: Write> {
    pub(super) font_system: &'fonts FontSystem,
    pub(super) path: PathBuf,
    pub(super) source: VideoInfo,
    pub(super) resume: ResumeTracker,
    pub(super) audio: AudioCatalog,
    pub(super) subtitles: SubtitleCatalog,
    pub(super) engine: PlaybackEngine,
    pub(super) view: PlaybackView<W>,
    pub(super) ui: PlaybackUi,
    pub(super) seeking: SeekCoordinator,
}

impl<'fonts, W: Write> PlaybackSession<'fonts, W> {
    pub(super) fn new(init: PlaybackSessionInit<'fonts, W>) -> Self {
        Self {
            font_system: init.font_system,
            path: init.path,
            source: init.source,
            resume: init.resume,
            audio: init.audio,
            subtitles: init.subtitles,
            engine: init.engine,
            view: init.view,
            ui: init.ui,
            seeking: init.seeking,
            resize: ResizeTracker::default(),
        }
    }
}

impl<W: Write> PlaybackSession<'_, W> {
    pub(super) fn run(self) -> Result<()> {
        let mut session = self;
        let playback_outcome = loop {
            if shutdown::requested() {
                break PlaybackOutcome::Interrupted;
            }
            session.poll_backends()?;

            let input = read_input_events()?;
            let input_at = Instant::now();
            session.poll_loaded_subtitles(input_at);
            session.note_mouse_activity(input.mouse_activity, input_at);
            session
                .interaction()
                .handle_text(input.text.as_deref(), input_at);
            if let Some(outcome) = session
                .interaction()
                .handle_command(input.command, input_at)?
            {
                break outcome;
            }

            session.advance_keyboard_preview();

            if session.reconcile_layout()? {
                continue;
            }

            if let Some(outcome) = session
                .interaction()
                .handle_pointer(input.mouse_events, input_at)?
            {
                break outcome;
            }

            session.commit_keyboard_seek()?;
            session.mark_expired_ui_dirty();

            if session.present_or_wait()? {
                continue;
            }

            if session.engine.complete() {
                break PlaybackOutcome::Completed;
            }
        };
        session.finish(playback_outcome)
    }

    fn poll_backends(&mut self) -> Result<()> {
        self.resume.maybe_checkpoint(Instant::now());
        if self.resume.take_error().is_some() {
            self.ui.status_message = Some(PlaybackUi::status("RESUME SAVE FAILED", Instant::now()));
        }
        self.engine.poll_audio()?;
        self.engine.sync_audio_clock();
        if progress_pending_seek(
            &mut self.seeking.pending,
            &self.engine.video,
            &mut self.engine.audio,
            self.engine.paused,
        ) {
            self.engine.next_frame_at = Instant::now();
        }
        Ok(())
    }

    fn poll_loaded_subtitles(&mut self, input_at: Instant) {
        while let Some(loaded) = self.subtitles.poll_loaded() {
            let (loaded_index, loaded_ok) = self.subtitles.apply_loaded(loaded);
            if self.subtitles.selected() != Some(loaded_index) {
                continue;
            }
            self.view.subtitle_renderer = SubtitleRenderer::new(
                self.font_system,
                self.subtitles.active().and_then(SubtitleTrack::language),
            );
            if !loaded_ok {
                self.ui.status_message = Some(PlaybackUi::status("SUBTITLE LOAD FAILED", input_at));
                self.ui.show_overlay(input_at);
            }
            self.view.dirty = self.view.have_frame;
        }
    }

    fn note_mouse_activity(&mut self, mouse_activity: bool, input_at: Instant) {
        if !mouse_activity {
            return;
        }
        let was_visible = self.ui.overlay_visible(
            self.engine.paused,
            self.seeking.scrub_position.is_some(),
            input_at,
        );
        self.ui.show_overlay(input_at);
        if self.view.have_frame && !was_visible {
            self.view.dirty = true;
        }
    }

    fn interaction(&mut self) -> InteractionContext<'_, W> {
        InteractionContext::new(
            self.font_system,
            &self.path,
            &self.source,
            &mut self.resume,
            &mut self.audio,
            &mut self.subtitles,
            &mut self.engine,
            &mut self.view,
            &mut self.ui,
            &mut self.seeking,
        )
    }

    fn advance_keyboard_preview(&mut self) {
        if advance_keyboard_seek_preview(
            &mut self.seeking.pending,
            &mut self.engine.video,
            self.seeking.scrub_position,
            self.seeking.keyboard_commit_at.is_some(),
        ) {
            self.engine.next_frame_at = Instant::now();
            self.view.dirty = false;
        }
    }

    fn reconcile_layout(&mut self) -> Result<bool> {
        let (observed_target, observed_canvas) =
            terminal_target_and_canvas(self.source.width, self.source.height);
        let resize_ready = self.resize.settled_change(
            self.view.target,
            self.view.canvas,
            observed_target,
            observed_canvas,
            Instant::now(),
        );
        let (current_target, current_canvas) =
            resize_ready.unwrap_or((self.view.target, self.view.canvas));
        if self.resize.is_pending() && resize_ready.is_none() {
            self.view.output.flush()?;
            thread::sleep(Duration::from_millis(8));
            self.resume.maybe_checkpoint(Instant::now());
            return Ok(true);
        }

        if current_target != self.view.target {
            let interrupted_seek = self.seeking.pending.take();
            let pending_resize_position = self
                .seeking
                .scrub_position
                .or_else(|| interrupted_seek.as_ref().map(|seek| seek.video_target));
            let resize_position = resize_restart_position(
                self.engine.position,
                self.source.duration,
                self.engine.paused,
                self.engine.audio.as_ref(),
                pending_resize_position,
            );

            self.view.frame.resize(current_target.frame_len(), 0);
            self.view.target = current_target;
            self.engine.restart_video(
                &self.path,
                self.view.target,
                self.source.fps,
                resize_position,
            )?;
            self.seeking.pending = Some(resize_pending_seek(
                self.engine.video.seek_generation(),
                resize_position,
                interrupted_seek,
            ));
            self.resume.set_position(self.engine.position);

            clear_screen_and_images(&mut self.view.output)?;
            self.view.reset_presented_frame();
            self.seeking.scrub_position = None;
            self.seeking.keyboard_commit_at = None;
        }

        if current_canvas != self.view.canvas {
            self.view.canvas = current_canvas;
            self.view
                .composited_frame
                .resize(self.view.canvas.frame_len(), 0);
            clear_screen_and_images(&mut self.view.output)?;
            self.view.reset_overlay_cache();
            if self.view.have_frame {
                self.render_current_frame()?;
            }
        }
        Ok(false)
    }

    fn commit_keyboard_seek(&mut self) -> Result<()> {
        if self
            .seeking
            .keyboard_commit_at
            .is_none_or(|deadline| Instant::now() < deadline)
        {
            return Ok(());
        }
        self.seeking.keyboard_commit_at = None;
        let Some(seek_target) = self.seeking.scrub_position.take() else {
            return Ok(());
        };
        if let Some(seek) = self.seeking.pending.as_mut() {
            if seek.needs_exact_retarget_for_release(seek_target) {
                seek.retarget_video(&mut self.engine.video, seek_target, true);
            }
            seek.request_release();
        } else {
            self.seeking.pending = Some(seek_playback(
                &self.path,
                self.source.has_audio,
                &mut self.engine.video,
                &mut self.engine.audio,
                &mut self.engine.audio_done,
                self.audio.choice(),
                seek_target,
                true,
                self.engine.paused,
                self.engine.muted,
            )?);
        }
        self.engine.video_ended = false;
        self.engine.next_frame_at = Instant::now();
        self.view.dirty = false;
        Ok(())
    }

    fn mark_expired_ui_dirty(&mut self) {
        let now = Instant::now();
        let overlay_visible = self.ui.overlay_visible(
            self.engine.paused,
            self.seeking.scrub_position.is_some(),
            now,
        );
        let status_visible = super::ui::status_text(self.ui.status_message, now).is_some();
        let media_info_visible = self.ui.media_info.visible(now);
        let media_info_fps_visible = media_info_visible
            && media_info_display_fps(self.engine.paused, self.engine.video.display_fps(now))
                .is_some();
        if self.view.have_frame
            && ((self.view.last_overlay_visible && !overlay_visible)
                || (self.view.last_status_visible && !status_visible)
                || (self.view.last_media_info_visible && !media_info_visible)
                || (self.view.last_media_info_fps_visible && !media_info_fps_visible))
        {
            self.view.dirty = true;
        }
    }

    fn present_or_wait(&mut self) -> Result<bool> {
        if self.view.dirty && self.view.have_frame {
            self.render_current_frame()?;
            self.view.output.flush()?;
        }

        if self.engine.paused {
            match self.engine.read_latest_frame(&mut self.view.frame)? {
                FrameStatus::NewFrame { pts } => {
                    self.engine.position = pts;
                    self.resume.set_position(self.engine.position);
                    self.render_current_frame()?;
                    mark_pending_seek_frame_displayed(&mut self.seeking.pending, pts);
                    advance_keyboard_seek_preview(
                        &mut self.seeking.pending,
                        &mut self.engine.video,
                        self.seeking.scrub_position,
                        self.seeking.keyboard_commit_at.is_some(),
                    );
                }
                FrameStatus::NoFrame => {}
                FrameStatus::Ended => self.engine.video_ended = true,
            }
            self.view.output.flush()?;
            thread::sleep(Duration::from_millis(15));
            self.resume.maybe_checkpoint(Instant::now());
            return Ok(true);
        }

        let now = Instant::now();
        if now < self.engine.next_frame_at {
            self.view.output.flush()?;
            thread::sleep((self.engine.next_frame_at - now).min(Duration::from_millis(5)));
            self.resume.maybe_checkpoint(Instant::now());
            return Ok(true);
        }

        match self.engine.read_latest_frame(&mut self.view.frame)? {
            FrameStatus::NewFrame { pts } => {
                self.engine.position = pts;
                self.resume.set_position(self.engine.position);
                self.render_current_frame()?;
                self.view.output.flush()?;
                mark_pending_seek_frame_displayed(&mut self.seeking.pending, pts);
                if advance_keyboard_seek_preview(
                    &mut self.seeking.pending,
                    &mut self.engine.video,
                    self.seeking.scrub_position,
                    self.seeking.keyboard_commit_at.is_some(),
                ) {
                    self.engine.next_frame_at = Instant::now();
                }
                self.engine.advance_frame_clock();
            }
            FrameStatus::NoFrame => {
                self.view.output.flush()?;
                thread::sleep(Duration::from_millis(2));
            }
            FrameStatus::Ended => {
                self.engine.video_ended = true;
                if let Some(duration) = self.source.duration {
                    self.engine.position = duration;
                    self.resume.set_position(self.engine.position);
                }
                if self.view.have_frame {
                    self.render_current_frame()?;
                }
                self.view.output.flush()?;
                thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(false)
    }

    fn render_current_frame(&mut self) -> Result<()> {
        let state = self.ui.state(
            self.engine.position,
            self.seeking.scrub_position,
            self.source.duration,
            self.engine.paused,
            &self.audio,
            &self.subtitles,
            self.view.canvas,
            &self.engine.video,
        );
        self.view.render(
            self.subtitles.active(),
            self.subtitles.selected().is_some(),
            self.engine.position,
            &state,
        )?;
        Ok(())
    }

    fn finish(mut self, outcome: PlaybackOutcome) -> Result<()> {
        let resume_result = if outcome.clears_resume() {
            self.resume.clear()
        } else {
            self.resume.save_now()
        };
        resume_result.context("failed to persist playback state")?;
        self.engine.stop()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
