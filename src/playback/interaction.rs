use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::{
    cli::{PlaybackCommand, PlaybackMouse},
    font::FontSystem,
    overlay::{
        AudioPickerAction, OverlayHitContext, PlaylistMenuAction, SubtitlePickerAction,
        TransportControlAction,
    },
    playlist::{PlaylistControls, PlaylistStep},
    resume::ResumeTracker,
    subtitle::{SubtitleRenderer, SubtitleTrack},
    video::VideoInfo,
};

use super::{
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
    playlist_controls: PlaylistControls,
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
        playlist_controls: PlaylistControls,
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
            playlist_controls,
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
            PlaybackCommand::QuitWithoutSaving => {
                return Ok(Some(PlaybackOutcome::QuitWithoutSaving));
            }
            _ => {}
        }
        if self.ui.help_visible {
            match command {
                PlaybackCommand::TogglePause => {
                    self.toggle_pause(input_at);
                }
                PlaybackCommand::ToggleHelp | PlaybackCommand::CloseTransientUi => {
                    self.release_keyboard_seek_preview()?;
                    self.close_transient_ui();
                }
                PlaybackCommand::SeekBySeconds {
                    picker_direction, ..
                } => self.scroll_help(picker_direction),
                PlaybackCommand::None => {}
                _ => {}
            }
            return Ok(None);
        }
        if self.ui.playlist_menu_open {
            match command {
                PlaybackCommand::TogglePause => self.toggle_pause(input_at),
                PlaybackCommand::TogglePlaylistMenu | PlaybackCommand::CloseTransientUi => {
                    self.close_playlist_menu();
                }
                PlaybackCommand::SeekBySeconds {
                    picker_direction, ..
                } => self.navigate_playlist_menu(picker_direction),
                PlaybackCommand::PlaylistPrevious => self.page_playlist_menu(-1),
                PlaybackCommand::PlaylistNext => self.page_playlist_menu(1),
                PlaybackCommand::PlaylistFirst => self.focus_playlist_boundary(false),
                PlaybackCommand::PlaylistLast => self.focus_playlist_boundary(true),
                PlaybackCommand::ConfirmPicker => {
                    return Ok(self.confirm_playlist_selection());
                }
                PlaybackCommand::None => {}
                _ => {}
            }
            return Ok(None);
        }

        match command {
            PlaybackCommand::Quit => return Ok(Some(PlaybackOutcome::Quit)),
            PlaybackCommand::QuitWithoutSaving => {
                return Ok(Some(PlaybackOutcome::QuitWithoutSaving));
            }
            PlaybackCommand::TogglePause => {
                self.toggle_pause(input_at);
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
            PlaybackCommand::PlaylistPrevious => {
                self.release_keyboard_seek_preview()?;
                if self.playlist_controls.previous_available {
                    return Ok(Some(PlaybackOutcome::Switch(PlaylistStep::Previous)));
                }
                self.show_playlist_status("FIRST VIDEO", input_at);
            }
            PlaybackCommand::PlaylistNext => {
                self.release_keyboard_seek_preview()?;
                if self.playlist_controls.next_available {
                    return Ok(Some(PlaybackOutcome::Switch(PlaylistStep::Next)));
                }
                self.show_playlist_status("LAST VIDEO", input_at);
            }
            PlaybackCommand::AdjustVolume { steps } => {
                self.adjust_volume(steps, input_at);
            }
            PlaybackCommand::ToggleSubtitles => {
                self.ui.subtitle_picker_open = false;
                if !self.subtitles.is_available() {
                    self.ui.status_message =
                        Some(PlaybackUi::status("NO SUBTITLES AVAILABLE", input_at));
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
                self.ui.help_visible = false;
                self.toggle_audio_picker(input_at);
            }
            PlaybackCommand::ToggleSubtitlePicker => {
                self.release_keyboard_seek_preview()?;
                self.ui.help_visible = false;
                self.toggle_subtitle_picker(input_at);
            }
            PlaybackCommand::TogglePlaylistMenu => {
                self.release_keyboard_seek_preview()?;
                self.toggle_playlist_menu(input_at);
            }
            PlaybackCommand::ShowMediaInfo => {
                self.ui.help_visible = false;
                self.ui.media_info.show(input_at);
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleMediaInfo => {
                self.ui.help_visible = false;
                self.ui.media_info.toggle();
                self.view.dirty = self.view.have_frame;
            }
            PlaybackCommand::ToggleHelp => {
                self.release_keyboard_seek_preview()?;
                self.toggle_help();
            }
            PlaybackCommand::CloseTransientUi => {
                self.release_keyboard_seek_preview()?;
                self.close_transient_ui();
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
                        self.engine.volume_percent,
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
            PlaybackCommand::PlaylistFirst | PlaybackCommand::PlaylistLast => {}
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
        if self.ui.help_visible {
            for mouse in mouse_events {
                match mouse {
                    PlaybackMouse::ScrollUp => self.scroll_help(-1),
                    PlaybackMouse::ScrollDown => self.scroll_help(1),
                    PlaybackMouse::Down { .. }
                        if close_help_on_outside_click(
                            &mut self.ui.help_visible,
                            &mut self.ui.help_scroll_offset,
                        ) =>
                    {
                        self.view.dirty = self.view.have_frame;
                    }
                    _ => {}
                }
            }
            return Ok(None);
        }
        if self.ui.playlist_menu_open {
            return Ok(self.handle_playlist_pointer(mouse_events));
        }

        let hit_context = self.overlay_hit_context();
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
                    if picker_owns_scroll(self.ui.audio_picker_open, self.ui.subtitle_picker_open) {
                        self.scroll_open_picker(hit_context, -1);
                    } else {
                        self.adjust_volume(1, input_at);
                    }
                    None
                }
                PlaybackMouse::ScrollDown => {
                    if picker_owns_scroll(self.ui.audio_picker_open, self.ui.subtitle_picker_open) {
                        self.scroll_open_picker(hit_context, 1);
                    } else {
                        self.adjust_volume(-1, input_at);
                    }
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
                    } else if let Some(action) = point.and_then(|point| {
                        self.view
                            .overlay
                            .transport_control_action(hit_context, point)
                    }) {
                        self.seeking.scrub_position = None;
                        self.ui.subtitle_picker_open = false;
                        self.ui.subtitle_picker_focus = None;
                        self.ui.audio_picker_open = false;
                        self.ui.audio_picker_focus = None;
                        self.ui.show_overlay(input_at);
                        self.release_keyboard_seek_preview()?;
                        match action {
                            TransportControlAction::Previous
                                if self.playlist_controls.previous_available =>
                            {
                                return Ok(Some(PlaybackOutcome::Switch(PlaylistStep::Previous)));
                            }
                            TransportControlAction::Next
                                if self.playlist_controls.next_available =>
                            {
                                return Ok(Some(PlaybackOutcome::Switch(PlaylistStep::Next)));
                            }
                            TransportControlAction::Previous => {
                                self.show_playlist_status("FIRST VIDEO", input_at)
                            }
                            TransportControlAction::Next => {
                                self.show_playlist_status("LAST VIDEO", input_at)
                            }
                            TransportControlAction::Playback => {
                                self.engine.toggle_pause(self.seeking.pending.is_some());
                            }
                        }
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
                self.engine.volume_percent,
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

    fn handle_playlist_pointer(
        &mut self,
        mouse_events: Vec<PlaybackMouse>,
    ) -> Option<PlaybackOutcome> {
        let context = self.overlay_hit_context();
        let labels = std::sync::Arc::clone(&self.ui.playlist_labels);
        for mouse in mouse_events {
            match mouse {
                PlaybackMouse::ScrollUp => self.scroll_playlist_menu(-1),
                PlaybackMouse::ScrollDown => self.scroll_playlist_menu(1),
                PlaybackMouse::Move { column, row } => {
                    let Some(point) = mouse_canvas_position(column, row, self.view.canvas) else {
                        continue;
                    };
                    let Some(index) = self.view.overlay.playlist_menu_hover_index(
                        context,
                        point,
                        self.ui.playlist_menu_offset,
                        &labels,
                    ) else {
                        continue;
                    };
                    if self.ui.playlist_menu_focus != Some(index) {
                        self.ui.playlist_menu_focus = Some(index);
                        self.view.dirty = self.view.have_frame;
                    }
                }
                PlaybackMouse::Down { column, row } => {
                    let action =
                        mouse_canvas_position(column, row, self.view.canvas).and_then(|point| {
                            self.view.overlay.playlist_menu_action(
                                context,
                                point,
                                self.ui.playlist_menu_offset,
                                &labels,
                            )
                        });
                    match action {
                        Some(PlaylistMenuAction::Close) => self.close_playlist_menu(),
                        Some(PlaylistMenuAction::Select(index)) => {
                            self.ui.playlist_menu_focus = Some(index);
                            return self.confirm_playlist_selection();
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn adjust_volume(&mut self, steps: i32, input_at: Instant) {
        let volume = self.engine.adjust_volume(steps);
        self.ui.status_message = Some(PlaybackUi::status(format!("VOLUME {volume}%"), input_at));
        self.ui.show_overlay(input_at);
        self.view.dirty = self.view.have_frame;
    }

    fn toggle_audio_picker(&mut self, input_at: Instant) {
        self.seeking.scrub_position = None;
        self.ui.help_visible = false;
        self.close_playlist_menu_state();
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
        self.ui.help_visible = false;
        self.close_playlist_menu_state();
        self.ui.show_overlay(input_at);
        if !self.subtitles.is_available() {
            self.ui.audio_picker_open = false;
            self.ui.audio_picker_focus = None;
            self.ui.subtitle_picker_open = false;
            self.ui.subtitle_picker_focus = None;
            self.ui.status_message = Some(PlaybackUi::status("NO SUBTITLES AVAILABLE", input_at));
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

    fn toggle_pause(&mut self, input_at: Instant) {
        self.ui.show_overlay(input_at);
        self.engine.toggle_pause(self.seeking.pending.is_some());
        self.view.dirty = self.view.have_frame;
    }

    fn toggle_playlist_menu(&mut self, input_at: Instant) {
        if self.ui.playlist_labels.len() <= 1 {
            self.close_playlist_menu_state();
            self.ui.audio_picker_open = false;
            self.ui.audio_picker_focus = None;
            self.ui.subtitle_picker_open = false;
            self.ui.subtitle_picker_focus = None;
            self.ui.status_message = Some(PlaybackUi::status("NO PLAYLIST AVAILABLE", input_at));
            self.ui.show_overlay(input_at);
            self.view.dirty = self.view.have_frame;
            return;
        }

        if self.ui.playlist_menu_open {
            self.close_playlist_menu();
            return;
        }

        self.ui.help_visible = false;
        self.ui.help_scroll_offset = 0;
        self.ui.audio_picker_open = false;
        self.ui.audio_picker_focus = None;
        self.ui.subtitle_picker_open = false;
        self.ui.subtitle_picker_focus = None;
        self.seeking.scrub_position = None;
        self.ui.playlist_menu_open = true;
        self.ui.playlist_menu_focus = Some(self.ui.playlist_current);
        let visible_count = self.visible_playlist_rows();
        self.ui.playlist_menu_offset = centered_picker_offset_for_focus(
            self.ui.playlist_current,
            self.ui.playlist_labels.len(),
            visible_count,
        );
        self.view.dirty = self.view.have_frame;
    }

    fn toggle_help(&mut self) {
        self.ui.help_visible = !self.ui.help_visible;
        if self.ui.help_visible {
            self.ui.help_scroll_offset = 0;
            self.ui.audio_picker_open = false;
            self.ui.audio_picker_focus = None;
            self.ui.subtitle_picker_open = false;
            self.ui.subtitle_picker_focus = None;
            self.close_playlist_menu_state();
            self.seeking.scrub_position = None;
        }
        self.view.dirty = self.view.have_frame;
    }

    fn close_transient_ui(&mut self) {
        let changed = transient_ui_is_visible(
            self.ui.help_visible,
            self.ui.playlist_menu_open,
            self.ui.audio_picker_open,
            self.ui.subtitle_picker_open,
            self.ui.audio_picker_focus,
            self.ui.subtitle_picker_focus,
            self.seeking.scrub_position.is_some(),
        );
        self.ui.help_visible = false;
        self.ui.help_scroll_offset = 0;
        self.ui.audio_picker_open = false;
        self.ui.audio_picker_focus = None;
        self.ui.subtitle_picker_open = false;
        self.ui.subtitle_picker_focus = None;
        self.close_playlist_menu_state();
        self.seeking.scrub_position = None;
        if changed {
            self.view.dirty = self.view.have_frame;
        }
    }

    fn scroll_help(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let context = self.overlay_hit_context();
        let max_offset = self.view.overlay.help_scroll_limit(context);
        let next = if direction < 0 {
            self.ui.help_scroll_offset.saturating_sub(1)
        } else {
            self.ui.help_scroll_offset.saturating_add(1).min(max_offset)
        };
        if next != self.ui.help_scroll_offset {
            self.ui.help_scroll_offset = next;
            self.view.dirty = self.view.have_frame;
        }
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
                self.engine.volume_percent,
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

    fn navigate_playlist_menu(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let row_count = self.ui.playlist_labels.len();
        let Some(next) = moved_picker_focus(self.ui.playlist_menu_focus, direction, row_count)
        else {
            return;
        };
        self.focus_playlist_index(next);
    }

    fn page_playlist_menu(&mut self, direction: i32) {
        if direction == 0 {
            return;
        }
        let row_count = self.ui.playlist_labels.len();
        if row_count == 0 {
            return;
        }
        let page = self.visible_playlist_rows().saturating_sub(1).max(1);
        let current = self
            .ui
            .playlist_menu_focus
            .unwrap_or(self.ui.playlist_current)
            .min(row_count - 1);
        let next = if direction < 0 {
            current.saturating_sub(page)
        } else {
            current.saturating_add(page).min(row_count - 1)
        };
        self.focus_playlist_index(next);
    }

    fn focus_playlist_boundary(&mut self, last: bool) {
        let row_count = self.ui.playlist_labels.len();
        if row_count == 0 {
            return;
        }
        self.focus_playlist_index(if last { row_count - 1 } else { 0 });
    }

    fn focus_playlist_index(&mut self, index: usize) {
        let row_count = self.ui.playlist_labels.len();
        let visible_count = self.visible_playlist_rows();
        self.ui.playlist_menu_focus = Some(index.min(row_count.saturating_sub(1)));
        self.ui.playlist_menu_offset = picker_offset_for_focus(
            self.ui.playlist_menu_offset,
            index,
            row_count,
            visible_count,
        );
        self.view.dirty = self.view.have_frame;
    }

    fn scroll_playlist_menu(&mut self, direction: i32) {
        let row_count = self.ui.playlist_labels.len();
        let visible_count = self.visible_playlist_rows();
        self.ui.playlist_menu_offset = scrolled_picker_offset(
            self.ui.playlist_menu_offset,
            direction,
            row_count,
            visible_count,
        );
        self.ui.playlist_menu_focus = keep_focus_visible(
            self.ui.playlist_menu_focus,
            self.ui.playlist_menu_offset,
            row_count,
            visible_count,
        );
        self.view.dirty = self.view.have_frame;
    }

    fn visible_playlist_rows(&mut self) -> usize {
        let context = self.overlay_hit_context();
        let labels = std::sync::Arc::clone(&self.ui.playlist_labels);
        self.view
            .overlay
            .playlist_menu_visible_row_count(context, &labels)
    }

    fn confirm_playlist_selection(&mut self) -> Option<PlaybackOutcome> {
        let index = self.ui.playlist_menu_focus?;
        self.close_playlist_menu();
        (index != self.ui.playlist_current).then_some(PlaybackOutcome::SelectPlaylistEntry(index))
    }

    fn close_playlist_menu(&mut self) {
        let changed = self.ui.playlist_menu_open || self.ui.playlist_menu_focus.is_some();
        self.close_playlist_menu_state();
        if changed {
            self.view.dirty = self.view.have_frame;
        }
    }

    fn close_playlist_menu_state(&mut self) {
        self.ui.playlist_menu_open = false;
        self.ui.playlist_menu_offset = 0;
        self.ui.playlist_menu_focus = None;
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
            self.engine.volume_percent,
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
        let context = self.overlay_hit_context();
        self.view
            .overlay
            .track_picker_visible_row_count(context, row_count)
    }

    fn overlay_hit_context(&self) -> OverlayHitContext {
        OverlayHitContext {
            width: self.view.canvas.width,
            height: self.view.canvas.height,
            terminal_cols: self.view.canvas.area.cols,
            terminal_rows: self.view.canvas.area.rows,
            scale_percent: self.view.canvas.overlay_scale_percent,
            position: self.seeking.scrub_position.unwrap_or(self.engine.position),
            duration: self.source.duration,
            audio_available: self.audio.is_available(),
            subtitles_available: self.subtitles.is_available(),
            playlist_previous_available: self.playlist_controls.previous_available,
            playlist_next_available: self.playlist_controls.next_available,
        }
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

    fn show_playlist_status(&mut self, text: &'static str, input_at: Instant) {
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

fn centered_picker_offset_for_focus(focus: usize, row_count: usize, visible_count: usize) -> usize {
    let visible_count = visible_count.max(1).min(row_count.max(1));
    focus
        .saturating_sub(visible_count / 2)
        .min(row_count.saturating_sub(visible_count))
}

fn picker_owns_scroll(audio_picker_open: bool, subtitle_picker_open: bool) -> bool {
    audio_picker_open || subtitle_picker_open
}

fn close_help_on_outside_click(help_visible: &mut bool, help_scroll_offset: &mut usize) -> bool {
    if !*help_visible {
        return false;
    }
    *help_visible = false;
    *help_scroll_offset = 0;
    true
}

fn transient_ui_is_visible(
    help_visible: bool,
    playlist_menu_open: bool,
    audio_picker_open: bool,
    subtitle_picker_open: bool,
    audio_picker_focus: Option<usize>,
    subtitle_picker_focus: Option<usize>,
    scrub_preview_visible: bool,
) -> bool {
    help_visible
        || playlist_menu_open
        || audio_picker_open
        || subtitle_picker_open
        || audio_picker_focus.is_some()
        || subtitle_picker_focus.is_some()
        || scrub_preview_visible
}

#[cfg(test)]
#[path = "tests/interaction.rs"]
mod tests;
