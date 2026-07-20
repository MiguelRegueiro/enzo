use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{
    font_system::FontSystem,
    media::VideoInfo,
    overlay::{AudioPickerAction, OverlayHitContext, SubtitlePickerAction},
    resume::ResumeTracker,
    subtitle::{SubtitleRenderer, SubtitleTrack},
};

use super::{
    super::terminal_input::{PlaybackCommand, PlaybackMouse},
    engine::PlaybackEngine,
    pointer::{canvas_position as mouse_canvas_position, canvas_x as mouse_canvas_x},
    resume_selection::{sync_resume_audio, sync_resume_subtitle},
    seek::{
        SeekCoordinator, is_end_seek, preview_playback, seek_from_progress_ratio, seek_playback,
        seek_position,
    },
    session::PlaybackOutcome,
    subtitles::{DroppedSubtitleSelection, SubtitleCatalog},
    tracks::AudioCatalog,
    ui::PlaybackUi,
    view::PlaybackView,
};

const KEYBOARD_SEEK_COMMIT_AFTER: Duration = Duration::from_millis(120);
const MOUSE_SCRUB_COMMIT_AFTER: Duration = Duration::from_millis(120);

#[derive(Clone, Copy)]
struct PointerSeekRequest {
    position: Duration,
    exact: bool,
}

impl PointerSeekRequest {
    fn preview(position: Duration) -> Self {
        Self {
            position,
            exact: false,
        }
    }

    fn exact(position: Duration) -> Self {
        Self {
            position,
            exact: true,
        }
    }
}

pub(super) struct InteractionContext<'a, W: Write> {
    font_system: &'a FontSystem,
    path: &'a Path,
    source: &'a VideoInfo,
    resume: &'a mut ResumeTracker,
    audio: &'a mut AudioCatalog,
    subtitles: &'a mut SubtitleCatalog,
    engine: &'a mut PlaybackEngine,
    view: &'a mut PlaybackView<W>,
    ui: &'a mut PlaybackUi,
    seeking: &'a mut SeekCoordinator,
}

impl<W: Write> InteractionContext<'_, W> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new<'a>(
        font_system: &'a FontSystem,
        path: &'a Path,
        source: &'a VideoInfo,
        resume: &'a mut ResumeTracker,
        audio: &'a mut AudioCatalog,
        subtitles: &'a mut SubtitleCatalog,
        engine: &'a mut PlaybackEngine,
        view: &'a mut PlaybackView<W>,
        ui: &'a mut PlaybackUi,
        seeking: &'a mut SeekCoordinator,
    ) -> InteractionContext<'a, W> {
        InteractionContext {
            font_system,
            path,
            source,
            resume,
            audio,
            subtitles,
            engine,
            view,
            ui,
            seeking,
        }
    }

    pub(super) fn handle_command(
        &mut self,
        command: PlaybackCommand,
        input_at: Instant,
    ) -> Result<Option<PlaybackOutcome>> {
        match command {
            PlaybackCommand::Quit => return Ok(Some(PlaybackOutcome::Quit)),
            PlaybackCommand::TogglePause => {
                self.ui.show_overlay(input_at);
                self.engine.toggle_pause(self.seeking.pending.is_some());
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleMute => {
                self.engine.toggle_mute();
                self.ui.status_message = Some(PlaybackUi::status(
                    if self.engine.muted {
                        "MUTE ON"
                    } else {
                        "MUTE OFF"
                    },
                    input_at,
                ));
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleSubtitles => {
                self.ui.subtitle_picker_open = false;
                if !self.subtitles.is_available() {
                    self.ui.status_message = Some(PlaybackUi::status("NO SUBTITLES", input_at));
                } else if self.subtitles.selected().is_some() {
                    self.subtitles.select(None);
                    sync_resume_subtitle(
                        self.resume,
                        self.path,
                        self.subtitles.tracks(),
                        self.subtitles.selected(),
                    );
                    self.ui.status_message = Some(PlaybackUi::status("SUBTITLES OFF", input_at));
                } else {
                    self.subtitles.select(Some(0));
                    sync_resume_subtitle(
                        self.resume,
                        self.path,
                        self.subtitles.tracks(),
                        self.subtitles.selected(),
                    );
                    self.view.subtitle_renderer = SubtitleRenderer::new(
                        self.font_system,
                        self.subtitles.active().and_then(SubtitleTrack::language),
                    );
                    self.ui.status_message = Some(PlaybackUi::status("SUBTITLES ON", input_at));
                }
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleAudioPicker => self.toggle_audio_picker(input_at),
            PlaybackCommand::ToggleSubtitlePicker => self.toggle_subtitle_picker(input_at),
            PlaybackCommand::ShowMediaInfo => {
                self.ui.media_info.show(input_at);
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleMediaInfo => {
                self.ui.media_info.toggle();
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::SeekBySeconds(seconds) => {
                let base_position = self.seeking.scrub_position.unwrap_or(self.engine.position);
                let seek_target = seek_position(base_position, seconds, self.source.duration);
                if is_end_seek(seek_target, self.source.duration) {
                    return Ok(Some(PlaybackOutcome::Completed));
                }
                self.ui.show_overlay(input_at);
                if self
                    .seeking
                    .keyboard_commit_at
                    .is_none_or(|deadline| input_at >= deadline)
                {
                    let mut seek = seek_playback(
                        self.path,
                        self.source.has_audio,
                        &mut self.engine.video,
                        &mut self.engine.audio,
                        &mut self.engine.audio_done,
                        self.audio.choice(),
                        seek_target,
                        true,
                        self.engine.paused,
                        self.engine.muted,
                    )?;
                    seek.hold();
                    self.seeking.pending = Some(seek);
                    self.engine.video_ended = false;
                    self.engine.next_frame_at = Instant::now();
                    self.view.dirty = false;
                } else {
                    self.view.dirty = self.view.have_frame;
                }
                self.seeking.scrub_position = Some(seek_target);
                self.seeking.keyboard_commit_at = Some(input_at + KEYBOARD_SEEK_COMMIT_AFTER);
            }
            PlaybackCommand::None => {}
        }
        Ok(None)
    }

    pub(super) fn handle_text(&mut self, text: Option<&str>, input_at: Instant) {
        let Some(text) = text else {
            return;
        };
        let status = match self.subtitles.select_from_drop_text(text) {
            DroppedSubtitleSelection::Ignored => return,
            DroppedSubtitleSelection::Failed => {
                self.show_subtitle_status("SUBTITLE LOAD FAILED", input_at);
                return;
            }
            DroppedSubtitleSelection::SelectedExisting => "SUBTITLES ALREADY LOADED",
            DroppedSubtitleSelection::Loaded => "SUBTITLES LOADED",
        };

        self.sync_subtitle_selection();
        self.ui.subtitle_picker_open = false;
        self.refresh_subtitle_renderer();
        self.show_subtitle_status(status, input_at);
    }

    pub(super) fn handle_pointer(
        &mut self,
        mouse_events: Vec<PlaybackMouse>,
        input_at: Instant,
    ) -> Result<Option<PlaybackOutcome>> {
        let hit_context = OverlayHitContext {
            width: self.view.canvas.width,
            height: self.view.canvas.height,
            terminal_rows: self.view.canvas.area.rows,
            scale_percent: self.view.canvas.overlay_scale_percent,
            position: self.seeking.scrub_position.unwrap_or(self.engine.position),
            duration: self.source.duration,
            audio_available: self.audio.is_available(),
            subtitles_available: self.subtitles.is_available(),
        };
        let audio_labels = self.audio.labels();
        let subtitle_labels = self.subtitles.labels();
        if !mouse_events.is_empty()
            && self.seeking.keyboard_commit_at.take().is_some()
            && let (Some(seek), Some(seek_target)) = (
                self.seeking.pending.as_mut(),
                self.seeking.scrub_position.take(),
            )
        {
            if seek.needs_exact_retarget_for_release(seek_target) {
                seek.retarget_video(&mut self.engine.video, seek_target, true);
            }
            seek.request_release();
            self.engine.next_frame_at = Instant::now();
        }

        let mut pointer_seek = None;
        for mouse in mouse_events {
            let seek = match mouse {
                PlaybackMouse::Down { column, row } => {
                    let point = mouse_canvas_position(column, row, self.view.canvas);
                    if let Some(action) = point.and_then(|point| {
                        self.view.overlay.audio_picker_action(
                            hit_context,
                            point,
                            self.ui.audio_picker_open,
                            &audio_labels,
                        )
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.show_overlay(input_at);
                        match action {
                            AudioPickerAction::TogglePicker => {
                                self.ui.audio_picker_open = !self.ui.audio_picker_open;
                                if self.ui.audio_picker_open {
                                    self.ui.subtitle_picker_open = false;
                                }
                            }
                            AudioPickerAction::SelectTrack(index) => {
                                self.audio.select(Some(index));
                                sync_resume_audio(
                                    self.resume,
                                    self.audio.tracks(),
                                    self.audio.selected(),
                                );
                                self.ui.audio_picker_open = false;
                                if let Some(mut player) = self.engine.audio.take() {
                                    player.stop()?;
                                }
                                self.engine.audio_done = true;
                                self.seeking.pending = Some(seek_playback(
                                    self.path,
                                    self.source.has_audio,
                                    &mut self.engine.video,
                                    &mut self.engine.audio,
                                    &mut self.engine.audio_done,
                                    self.audio.choice(),
                                    self.engine.position,
                                    true,
                                    self.engine.paused,
                                    self.engine.muted,
                                )?);
                            }
                        }
                        self.view.dirty = self.view.have_frame;
                    } else if let Some(action) = point.and_then(|point| {
                        self.view.overlay.subtitle_picker_action(
                            hit_context,
                            point,
                            self.ui.subtitle_picker_open,
                            &subtitle_labels,
                        )
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.show_overlay(input_at);
                        match action {
                            SubtitlePickerAction::TogglePicker => {
                                self.ui.subtitle_picker_open = !self.ui.subtitle_picker_open;
                                if self.ui.subtitle_picker_open {
                                    self.ui.audio_picker_open = false;
                                }
                            }
                            SubtitlePickerAction::SelectTrack(index) => {
                                self.subtitles.select(Some(index));
                                self.sync_subtitle_selection();
                                self.ui.subtitle_picker_open = false;
                                self.refresh_subtitle_renderer();
                                if self.subtitles.active().is_none() {
                                    self.show_subtitle_status("SUBTITLE LOADING", input_at);
                                }
                            }
                            SubtitlePickerAction::SelectOff => {
                                self.subtitles.select(None);
                                self.sync_subtitle_selection();
                                self.ui.subtitle_picker_open = false;
                            }
                        }
                        self.view.dirty = self.view.have_frame;
                    } else if point.is_some_and(|point| {
                        self.view
                            .overlay
                            .playback_button_hit_test(hit_context, point)
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.subtitle_picker_open = false;
                        self.ui.audio_picker_open = false;
                        self.ui.show_overlay(input_at);
                        self.engine.toggle_pause(self.seeking.pending.is_some());
                        self.view.dirty = self.view.have_frame;
                    } else {
                        let picker_was_open =
                            self.ui.audio_picker_open || self.ui.subtitle_picker_open;
                        self.ui.audio_picker_open = false;
                        self.ui.subtitle_picker_open = false;
                        self.seeking.scrub_position = point
                            .and_then(|point| {
                                self.view.overlay.progress_hit_test(hit_context, point)
                            })
                            .and_then(|ratio| {
                                seek_from_progress_ratio(ratio, self.source.duration)
                            });
                        self.seeking.mouse_commit_at = self
                            .seeking
                            .scrub_position
                            .map(|_| input_at + MOUSE_SCRUB_COMMIT_AFTER);
                        if picker_was_open || self.seeking.scrub_position.is_some() {
                            self.view.dirty = self.view.have_frame;
                        }
                    }
                    None
                }
                PlaybackMouse::Drag { column, row } if self.seeking.scrub_position.is_some() => {
                    let x = mouse_canvas_x(column, row, self.view.canvas);
                    let ratio = self.view.overlay.progress_ratio_from_x(hit_context, x);
                    self.seeking.scrub_position =
                        seek_from_progress_ratio(ratio, self.source.duration);
                    self.view.dirty = self.view.have_frame;
                    if self
                        .seeking
                        .mouse_commit_at
                        .is_some_and(|deadline| input_at >= deadline)
                    {
                        self.seeking.mouse_commit_at = Some(input_at + MOUSE_SCRUB_COMMIT_AFTER);
                        self.seeking.scrub_position.map(PointerSeekRequest::preview)
                    } else {
                        None
                    }
                }
                PlaybackMouse::Up { column, row } if self.seeking.scrub_position.is_some() => {
                    let x = mouse_canvas_x(column, row, self.view.canvas);
                    let ratio = self.view.overlay.progress_ratio_from_x(hit_context, x);
                    let target = seek_from_progress_ratio(ratio, self.source.duration);
                    self.seeking.scrub_position = None;
                    self.seeking.mouse_commit_at = None;
                    target.map(PointerSeekRequest::exact)
                }
                PlaybackMouse::Up { .. } => {
                    self.seeking.scrub_position = None;
                    self.seeking.mouse_commit_at = None;
                    None
                }
                _ => None,
            };

            if let Some(seek) = seek {
                pointer_seek = Some(seek);
            }
        }

        let Some(seek) = pointer_seek else {
            return Ok(None);
        };
        self.seeking.keyboard_commit_at = None;
        if is_end_seek(seek.position, self.source.duration) {
            return Ok(Some(PlaybackOutcome::Completed));
        }
        if seek.exact {
            self.seeking.pending = Some(seek_playback(
                self.path,
                self.source.has_audio,
                &mut self.engine.video,
                &mut self.engine.audio,
                &mut self.engine.audio_done,
                self.audio.choice(),
                seek.position,
                true,
                self.engine.paused,
                self.engine.muted,
            )?);
        } else {
            preview_playback(
                &mut self.engine.video,
                self.engine.audio.as_ref(),
                &mut self.seeking.pending,
                seek.position,
            );
        }
        self.engine.position = seek.position;
        self.resume.set_position(self.engine.position);
        self.engine.video_ended = false;
        self.engine.next_frame_at = Instant::now();
        self.view.dirty = false;
        Ok(None)
    }

    fn toggle_audio_picker(&mut self, input_at: Instant) {
        self.seeking.scrub_position = None;
        self.ui.show_overlay(input_at);
        if !self.audio.is_available() {
            self.ui.audio_picker_open = false;
            self.ui.subtitle_picker_open = false;
            self.ui.status_message = Some(PlaybackUi::status("NO AUDIO TRACKS", input_at));
        } else {
            self.ui.audio_picker_open = !self.ui.audio_picker_open;
            if self.ui.audio_picker_open {
                self.ui.subtitle_picker_open = false;
            }
        }
        self.view.dirty = self.view.have_frame;
    }

    fn toggle_subtitle_picker(&mut self, input_at: Instant) {
        self.seeking.scrub_position = None;
        self.ui.show_overlay(input_at);
        if !self.subtitles.is_available() {
            self.ui.audio_picker_open = false;
            self.ui.subtitle_picker_open = false;
            self.ui.status_message = Some(PlaybackUi::status("NO SUBTITLES", input_at));
        } else {
            self.ui.subtitle_picker_open = !self.ui.subtitle_picker_open;
            if self.ui.subtitle_picker_open {
                self.ui.audio_picker_open = false;
            }
        }
        self.view.dirty = self.view.have_frame;
    }

    fn sync_subtitle_selection(&mut self) {
        sync_resume_subtitle(
            self.resume,
            self.path,
            self.subtitles.tracks(),
            self.subtitles.selected(),
        );
    }

    fn refresh_subtitle_renderer(&mut self) {
        self.view.subtitle_renderer = SubtitleRenderer::new(
            self.font_system,
            self.subtitles.active().and_then(SubtitleTrack::language),
        );
    }

    fn show_subtitle_status(&mut self, text: &'static str, input_at: Instant) {
        self.ui.status_message = Some(PlaybackUi::status(text, input_at));
        self.ui.show_overlay(input_at);
        self.view.dirty = self.view.have_frame;
    }
}
