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
            PlaybackCommand::ToggleAudioPicker => {
                self.release_keyboard_seek_preview()?;
                self.toggle_audio_picker(input_at);
            }
            PlaybackCommand::ToggleSubtitlePicker => {
                self.release_keyboard_seek_preview()?;
                self.toggle_subtitle_picker(input_at);
            }
            PlaybackCommand::ShowMediaInfo => {
                self.ui.media_info.show(input_at);
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleMediaInfo => {
                self.ui.media_info.toggle();
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::SeekBySeconds {
                seconds,
                picker_direction,
            } => {
                if self.ui.audio_picker_open || self.ui.subtitle_picker_open {
                    if picker_direction != 0 && self.navigate_open_picker(picker_direction) {
                        self.ui.show_overlay(input_at);
                        self.view.dirty = self.view.have_frame;
                    }
                    return Ok(None);
                }
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
            PlaybackCommand::ConfirmPicker => self.confirm_open_picker(input_at)?,
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
        if mouse_events
            .iter()
            .any(|mouse| mouse.interrupts_keyboard_seek())
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
                PlaybackMouse::ScrollUp => {
                    self.scroll_open_picker(hit_context, -1);
                    None
                }
                PlaybackMouse::ScrollDown => {
                    self.scroll_open_picker(hit_context, 1);
                    None
                }
                PlaybackMouse::Down { column, row } => {
                    let point = mouse_canvas_position(column, row, self.view.canvas);
                    if let Some(action) = point.and_then(|point| {
                        self.view.overlay.audio_picker_action(
                            hit_context,
                            point,
                            self.ui.audio_picker_open,
                            self.ui.audio_picker_offset,
                            &audio_labels,
                        )
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.show_overlay(input_at);
                        match action {
                            AudioPickerAction::TogglePicker => self.toggle_audio_picker(input_at),
                            AudioPickerAction::SelectTrack(index) => {
                                self.select_audio_track(index)?
                            }
                        }
                        self.view.dirty = self.view.have_frame;
                    } else if let Some(action) = point.and_then(|point| {
                        self.view.overlay.subtitle_picker_action(
                            hit_context,
                            point,
                            self.ui.subtitle_picker_open,
                            self.ui.subtitle_picker_offset,
                            &subtitle_labels,
                        )
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.show_overlay(input_at);
                        match action {
                            SubtitlePickerAction::TogglePicker => {
                                self.toggle_subtitle_picker(input_at)
                            }
                            SubtitlePickerAction::SelectTrack(index) => {
                                self.select_subtitle_track(index, input_at)
                            }
                            SubtitlePickerAction::SelectOff => self.select_subtitle_off(),
                        }
                        self.view.dirty = self.view.have_frame;
                    } else if point.is_some_and(|point| {
                        self.view
                            .overlay
                            .playback_button_hit_test(hit_context, point)
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.subtitle_picker_open = false;
                        self.ui.subtitle_picker_focus = None;
                        self.ui.audio_picker_open = false;
                        self.ui.audio_picker_focus = None;
                        self.ui.show_overlay(input_at);
                        self.engine.toggle_pause(self.seeking.pending.is_some());
                        self.view.dirty = self.view.have_frame;
                    } else {
                        let picker_was_open =
                            self.ui.audio_picker_open || self.ui.subtitle_picker_open;
                        self.ui.audio_picker_open = false;
                        self.ui.audio_picker_focus = None;
                        self.ui.subtitle_picker_open = false;
                        self.ui.subtitle_picker_focus = None;
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
                PlaybackMouse::Move { column, row } => {
                    if let Some(point) = mouse_canvas_position(column, row, self.view.canvas) {
                        self.hover_open_picker(hit_context, point, &audio_labels, &subtitle_labels);
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

    fn scroll_open_picker(&mut self, context: OverlayHitContext, direction: i32) {
        if self.ui.audio_picker_open {
            let row_count = self.audio.labels().len();
            let visible_count = self
                .view
                .overlay
                .track_picker_visible_row_count(context, row_count);
            self.ui.audio_picker_offset = scrolled_picker_offset(
                self.ui.audio_picker_offset,
                direction,
                row_count,
                visible_count,
            );
            self.ui.audio_picker_focus = keep_focus_visible(
                self.ui.audio_picker_focus,
                self.ui.audio_picker_offset,
                row_count,
                visible_count,
            );
            self.view.dirty = self.view.have_frame;
        } else if self.ui.subtitle_picker_open {
            let row_count = self.subtitles.labels().len().saturating_add(1);
            let visible_count = self
                .view
                .overlay
                .track_picker_visible_row_count(context, row_count);
            self.ui.subtitle_picker_offset = scrolled_picker_offset(
                self.ui.subtitle_picker_offset,
                direction,
                row_count,
                visible_count,
            );
            self.ui.subtitle_picker_focus = keep_focus_visible(
                self.ui.subtitle_picker_focus,
                self.ui.subtitle_picker_offset,
                row_count,
                visible_count,
            );
            self.view.dirty = self.view.have_frame;
        }
    }

    fn toggle_audio_picker(&mut self, input_at: Instant) {
        self.seeking.scrub_position = None;
        self.ui.show_overlay(input_at);
        if !self.audio.is_available() {
            self.ui.audio_picker_open = false;
            self.ui.audio_picker_focus = None;
            self.ui.subtitle_picker_open = false;
            self.ui.subtitle_picker_focus = None;
            self.ui.status_message = Some(PlaybackUi::status("NO AUDIO TRACKS", input_at));
        } else {
            self.ui.audio_picker_open = !self.ui.audio_picker_open;
            if self.ui.audio_picker_open {
                let row_count = self.audio.labels().len();
                let focus = self
                    .audio
                    .selected()
                    .unwrap_or(0)
                    .min(row_count.saturating_sub(1));
                self.ui.audio_picker_focus = (row_count > 0).then_some(focus);
                self.ui.audio_picker_offset = picker_offset_for_focus(
                    0,
                    focus,
                    row_count,
                    self.visible_picker_rows(row_count),
                );
                self.ui.subtitle_picker_open = false;
                self.ui.subtitle_picker_focus = None;
            } else {
                self.ui.audio_picker_focus = None;
            }
        }
        self.view.dirty = self.view.have_frame;
    }

    fn toggle_subtitle_picker(&mut self, input_at: Instant) {
        self.seeking.scrub_position = None;
        self.ui.show_overlay(input_at);
        if !self.subtitles.is_available() {
            self.ui.audio_picker_open = false;
            self.ui.audio_picker_focus = None;
            self.ui.subtitle_picker_open = false;
            self.ui.subtitle_picker_focus = None;
            self.ui.status_message = Some(PlaybackUi::status("NO SUBTITLES", input_at));
        } else {
            self.ui.subtitle_picker_open = !self.ui.subtitle_picker_open;
            if self.ui.subtitle_picker_open {
                let row_count = self.subtitles.labels().len().saturating_add(1);
                let focus = self
                    .subtitles
                    .selected()
                    .unwrap_or_else(|| row_count.saturating_sub(1));
                self.ui.subtitle_picker_focus = (row_count > 0).then_some(focus);
                self.ui.subtitle_picker_offset = picker_offset_for_focus(
                    0,
                    focus,
                    row_count,
                    self.visible_picker_rows(row_count),
                );
                self.ui.audio_picker_open = false;
                self.ui.audio_picker_focus = None;
            } else {
                self.ui.subtitle_picker_focus = None;
            }
        }
        self.view.dirty = self.view.have_frame;
    }

    fn release_keyboard_seek_preview(&mut self) -> Result<()> {
        if self.seeking.keyboard_commit_at.take().is_none() {
            return Ok(());
        }
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
            )?);
        }
        self.engine.video_ended = false;
        self.engine.next_frame_at = Instant::now();
        self.view.dirty = false;
        Ok(())
    }

    fn navigate_open_picker(&mut self, direction: i32) -> bool {
        if direction == 0 {
            return false;
        }
        if self.ui.audio_picker_open {
            let row_count = self.audio.labels().len();
            let Some(next) = moved_picker_focus(self.ui.audio_picker_focus, direction, row_count)
            else {
                return false;
            };
            self.ui.audio_picker_focus = Some(next);
            self.ui.audio_picker_offset = picker_offset_for_focus(
                self.ui.audio_picker_offset,
                next,
                row_count,
                self.visible_picker_rows(row_count),
            );
            true
        } else if self.ui.subtitle_picker_open {
            let row_count = self.subtitles.labels().len().saturating_add(1);
            let Some(next) =
                moved_picker_focus(self.ui.subtitle_picker_focus, direction, row_count)
            else {
                return false;
            };
            self.ui.subtitle_picker_focus = Some(next);
            self.ui.subtitle_picker_offset = picker_offset_for_focus(
                self.ui.subtitle_picker_offset,
                next,
                row_count,
                self.visible_picker_rows(row_count),
            );
            true
        } else {
            false
        }
    }

    fn confirm_open_picker(&mut self, input_at: Instant) -> Result<()> {
        if self.ui.audio_picker_open
            && let Some(index) = self.ui.audio_picker_focus
        {
            self.select_audio_track(index)?;
            self.view.dirty = self.view.have_frame;
        } else if self.ui.subtitle_picker_open
            && let Some(index) = self.ui.subtitle_picker_focus
        {
            if index < self.subtitles.labels().len() {
                self.select_subtitle_track(index, input_at);
            } else {
                self.select_subtitle_off();
            }
            self.view.dirty = self.view.have_frame;
        }
        Ok(())
    }

    fn hover_open_picker(
        &mut self,
        context: OverlayHitContext,
        point: crate::overlay::OverlayHitPoint,
        audio_labels: &[std::sync::Arc<str>],
        subtitle_labels: &[std::sync::Arc<str>],
    ) {
        if self.ui.audio_picker_open
            && let Some(index) = self.view.overlay.audio_picker_hover_index(
                context,
                point,
                true,
                self.ui.audio_picker_offset,
                audio_labels,
            )
            && self.ui.audio_picker_focus != Some(index)
        {
            self.ui.audio_picker_focus = Some(index);
            self.view.dirty = self.view.have_frame;
        } else if self.ui.subtitle_picker_open
            && let Some(index) = self.view.overlay.subtitle_picker_hover_index(
                context,
                point,
                true,
                self.ui.subtitle_picker_offset,
                subtitle_labels,
            )
            && self.ui.subtitle_picker_focus != Some(index)
        {
            self.ui.subtitle_picker_focus = Some(index);
            self.view.dirty = self.view.have_frame;
        }
    }

    fn select_audio_track(&mut self, index: usize) -> Result<()> {
        if index >= self.audio.labels().len() {
            return Ok(());
        }
        self.audio.select(Some(index));
        sync_resume_audio(self.resume, self.audio.tracks(), self.audio.selected());
        self.ui.audio_picker_open = false;
        self.ui.audio_picker_focus = None;
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
        Ok(())
    }

    fn select_subtitle_track(&mut self, index: usize, input_at: Instant) {
        if index >= self.subtitles.labels().len() {
            return;
        }
        self.subtitles.select(Some(index));
        self.sync_subtitle_selection();
        self.ui.subtitle_picker_open = false;
        self.ui.subtitle_picker_focus = None;
        self.refresh_subtitle_renderer();
        if self.subtitles.active().is_none() {
            self.show_subtitle_status("SUBTITLE LOADING", input_at);
        }
    }

    fn select_subtitle_off(&mut self) {
        self.subtitles.select(None);
        self.sync_subtitle_selection();
        self.ui.subtitle_picker_open = false;
        self.ui.subtitle_picker_focus = None;
    }

    fn visible_picker_rows(&mut self, row_count: usize) -> usize {
        let context = OverlayHitContext {
            width: self.view.canvas.width,
            height: self.view.canvas.height,
            terminal_rows: self.view.canvas.area.rows,
            scale_percent: self.view.canvas.overlay_scale_percent,
            position: self.seeking.scrub_position.unwrap_or(self.engine.position),
            duration: self.source.duration,
            audio_available: self.audio.is_available(),
            subtitles_available: self.subtitles.is_available(),
        };
        self.view
            .overlay
            .track_picker_visible_row_count(context, row_count)
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

fn scrolled_picker_offset(
    offset: usize,
    direction: i32,
    row_count: usize,
    visible_count: usize,
) -> usize {
    let max_offset = row_count.saturating_sub(visible_count.max(1));
    if direction < 0 {
        offset.saturating_sub(1)
    } else {
        offset.saturating_add(1).min(max_offset)
    }
}

fn moved_picker_focus(current: Option<usize>, direction: i32, row_count: usize) -> Option<usize> {
    if row_count == 0 {
        return None;
    }
    let current = current.unwrap_or(0).min(row_count - 1);
    if direction < 0 {
        Some(current.saturating_sub(1))
    } else {
        Some(current.saturating_add(1).min(row_count - 1))
    }
}

fn keep_focus_visible(
    focus: Option<usize>,
    offset: usize,
    row_count: usize,
    visible_count: usize,
) -> Option<usize> {
    let visible_count = visible_count.max(1).min(row_count.max(1));
    let last_visible = offset
        .saturating_add(visible_count)
        .min(row_count)
        .saturating_sub(1);
    let focus = focus?.min(row_count.checked_sub(1)?);
    Some(focus.clamp(offset, last_visible))
}

fn picker_offset_for_focus(
    offset: usize,
    focus: usize,
    row_count: usize,
    visible_count: usize,
) -> usize {
    let visible_count = visible_count.max(1).min(row_count.max(1));
    let max_offset = row_count.saturating_sub(visible_count);
    if focus < offset {
        focus.min(max_offset)
    } else if focus >= offset.saturating_add(visible_count) {
        focus
            .saturating_add(1)
            .saturating_sub(visible_count)
            .min(max_offset)
    } else {
        offset.min(max_offset)
    }
}
