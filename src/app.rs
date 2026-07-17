mod cli;
mod layout;
mod resume_integration;
mod subtitle_tracks;

use std::{
    env, fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

use crate::{
    drop_target::draw_drop_target,
    font_system::FontSystem,
    input::{DropCommand, PlaybackCommand, PlaybackMouse, read_drop_events, read_input_events},
    media::{AudioPlayer, FrameStatus, VideoDecoder, VideoInfo, load_audio_tracks, probe_video},
    overlay::{
        AudioPickerAction, HitboxRect, MediaInfo, MediaInfoState, OverlayHitContext,
        OverlayHitPoint, OverlayState, PlaybackOverlay, SubtitlePickerAction,
    },
    resume::ResumeTracker,
    shutdown,
    subtitle::{SubtitleRenderer, SubtitleTrack},
    terminal::{
        KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, TerminalGuard,
        clear_screen_and_images, enable_tmux_passthrough, inside_tmux, looks_like_kitty,
        write_kitty_rgb_frame,
    },
};

use cli::{media_path_from_drop_text, parse_args};
use layout::{CanvasFrame, TargetFrame, terminal_target_and_canvas};
use resume_integration::{
    restore_audio_selection, restore_subtitle_selection, resume_available,
    selected_audio_stream_choice, sync_resume_audio, sync_resume_subtitle,
};
use subtitle_tracks::{
    PlaybackSubtitleTrack, active_subtitle_track, external_subtitle_indices,
    initial_external_subtitle_paths, load_dropped_subtitle_track, load_initial_subtitle_tracks,
    normalized_subtitle_path, spawn_embedded_subtitle_loader, subtitle_labels,
    subtitle_path_from_drop_text,
};

const OVERLAY_VISIBLE_FOR: Duration = Duration::from_secs(2);
const STATUS_VISIBLE_FOR: Duration = Duration::from_secs(2);
const MEDIA_INFO_VISIBLE_FOR: Duration = Duration::from_secs(4);
const KEYBOARD_SEEK_COMMIT_AFTER: Duration = Duration::from_millis(120);
const MOUSE_SCRUB_COMMIT_AFTER: Duration = Duration::from_millis(120);
const RESIZE_SETTLE_FOR: Duration = Duration::from_millis(140);

#[derive(Clone, Copy)]
struct StatusMessage {
    text: &'static str,
    visible_until: Instant,
}

struct PendingSeek {
    video_generation: i32,
    video_target: Duration,
    video_pts: Option<Duration>,
    video_frame_displayed: bool,
    audio_generation: Option<i32>,
    audio_target: Option<Duration>,
    release_requested: bool,
}

impl PendingSeek {
    fn hold(&mut self) {
        self.release_requested = false;
    }

    fn request_release(&mut self) {
        self.release_requested = true;
    }

    fn needs_exact_retarget_for_release(&self, position: Duration) -> bool {
        self.video_target != position || !self.release_requested
    }

    fn retarget_video(&mut self, decoder: &mut VideoDecoder, position: Duration, exact: bool) {
        self.video_generation = if exact {
            decoder.seek(position)
        } else {
            decoder.preview_seek(position)
        };
        self.video_target = position;
        self.video_pts = None;
        self.video_frame_displayed = false;
    }

    fn mark_video_frame_displayed(&mut self, pts: Duration) {
        if self.video_pts.is_none() {
            self.video_pts = Some(pts);
        }
        if self.video_pts == Some(pts) {
            self.video_frame_displayed = true;
        }
    }
}

struct MediaInfoOverlay {
    content: MediaInfo,
    visible_until: Option<Instant>,
    pinned: bool,
}

#[derive(Clone, Copy)]
struct PendingResize {
    target: TargetFrame,
    canvas: CanvasFrame,
    observed_at: Instant,
}

impl MediaInfoOverlay {
    fn new(content: MediaInfo) -> Self {
        Self {
            content,
            visible_until: None,
            pinned: false,
        }
    }

    fn show(&mut self, now: Instant) {
        self.visible_until = Some(now + MEDIA_INFO_VISIBLE_FOR);
    }

    fn toggle(&mut self) {
        self.pinned = !self.pinned;
        self.visible_until = None;
    }

    fn visible(&self, now: Instant) -> bool {
        self.pinned || self.visible_until.is_some_and(|deadline| now < deadline)
    }

    fn state(
        &self,
        selected_audio: Option<usize>,
        canvas: CanvasFrame,
        decoder: &VideoDecoder,
        paused: bool,
        now: Instant,
    ) -> Option<MediaInfoState> {
        if !self.visible(now) {
            return None;
        }
        Some(MediaInfoState {
            info: self.content.clone(),
            selected_audio,
            display_width: canvas.video_width,
            display_height: canvas.video_height,
            display_paused: paused,
            display_fps: media_info_display_fps(paused, decoder.display_fps(now)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackOutcome {
    Quit,
    Completed,
    Interrupted,
}

pub(crate) fn run() -> Result<()> {
    let config = parse_args(env::args_os().skip(1))?;
    if config.clear_resume {
        let removed = ResumeTracker::clear_all().context("failed to clear saved playback state")?;
        println!("Cleared {removed} saved playback state file(s).");
        return Ok(());
    }
    shutdown::install_signal_handlers().context("failed to install shutdown handlers")?;
    let font_system = FontSystem::discover();
    if !config.force && !looks_like_kitty() {
        bail!(
            "Enzo targets Kitty graphics; run from kitty or pass --force if your terminal is compatible"
        );
    }

    if inside_tmux() {
        enable_tmux_passthrough();
    }

    if let Some(path) = config.path {
        let _terminal = TerminalGuard::enter()?;
        play_media(
            path,
            config.sub_file.as_deref(),
            config.resume_enabled,
            &font_system,
        )
    } else {
        run_drop_target(
            config.sub_file.as_deref(),
            config.resume_enabled,
            &font_system,
        )
    }
}

fn run_drop_target(
    sub_file: Option<&Path>,
    resume_enabled: bool,
    font_system: &FontSystem,
) -> Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut status = None::<String>;

    loop {
        if shutdown::requested() {
            return Ok(());
        }
        draw_drop_target(&mut out, status.as_deref())?;
        let input = read_drop_events()?;
        if shutdown::requested() {
            return Ok(());
        }
        if input.command == DropCommand::Quit {
            return Ok(());
        }
        let Some(text) = input.text else {
            continue;
        };

        match media_path_from_drop_text(&text) {
            Ok(path) => {
                clear_screen_and_images(&mut out)?;
                out.flush()?;
                drop(out);
                return play_media(path, sub_file, resume_enabled, font_system);
            }
            Err(error) => {
                status = Some(error.to_string());
            }
        }
    }
}

fn play_media(
    path: PathBuf,
    sub_file: Option<&Path>,
    resume_enabled: bool,
    font_system: &FontSystem,
) -> Result<()> {
    let source = probe_video(&path)
        .with_context(|| format!("failed to inspect video metadata for {}", path.display()))?;
    let mut resume = ResumeTracker::open(
        &path,
        source.duration,
        resume_available(resume_enabled, source.seekable),
    );
    let restored = resume.restored().cloned();
    let (initial_subtitle_paths, mut restored_external_subtitle_missing) =
        initial_external_subtitle_paths(&path, sub_file, restored.as_ref());
    let initial_subtitles = load_initial_subtitle_tracks(&path, &initial_subtitle_paths)?;
    restored_external_subtitle_missing |= initial_subtitles.restored_external_load_failed;
    let mut subtitle_tracks = initial_subtitles.tracks;
    let mut subtitle_labels = subtitle_labels(&subtitle_tracks);
    let mut selected_subtitle = restore_subtitle_selection(
        &subtitle_tracks,
        restored.as_ref(),
        initial_subtitles.restored_external_index,
    )
    .unwrap_or_else(|| (!subtitle_tracks.is_empty()).then_some(0_usize));
    let mut external_subtitle_paths = external_subtitle_indices(&subtitle_tracks);
    let embedded_subtitle_rx =
        spawn_embedded_subtitle_loader(path.clone(), initial_subtitles.embedded_jobs);
    let mut audio_tracks = load_audio_tracks(&path);
    if audio_tracks.is_empty() && source.has_audio {
        audio_tracks.push(crate::media::AudioTrack::default_track());
    }
    let audio_labels = audio_tracks
        .iter()
        .map(|track| Box::leak(track.label().to_string().into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    let mut media_info = MediaInfoOverlay::new(MediaInfo::new(
        file_info_summary(&path, &source),
        source.source_summary(),
        audio_tracks
            .iter()
            .map(crate::media::AudioTrack::playback_summary)
            .collect(),
    ));
    let mut selected_audio = restore_audio_selection(&audio_tracks, restored.as_ref())
        .unwrap_or_else(|| (!audio_tracks.is_empty()).then_some(0_usize));
    let mut audio_picker_open = false;
    let mut subtitle_picker_open = false;
    let media_title = media_title(&path);
    let (mut target, mut canvas) = terminal_target_and_canvas(source.width, source.height);
    let start_position = restored
        .as_ref()
        .and_then(|restored| restored.position)
        .unwrap_or(Duration::ZERO);

    let mut decoder = VideoDecoder::spawn_at(
        &path,
        target.width,
        target.height,
        source.fps,
        start_position,
        true,
    )?;
    let mut muted = false;
    let selected_audio_stream = selected_audio_stream_choice(&audio_tracks, selected_audio);
    let mut audio = if source.has_audio {
        selected_audio_stream
            .map(|stream_index| {
                AudioPlayer::spawn_held_at(&path, stream_index, start_position, false, muted)
            })
            .transpose()?
    } else {
        None
    };
    decoder.set_audio_clock(audio.as_ref());
    let mut audio_done = !source.has_audio || selected_audio_stream.is_none();
    let playback_started_at = Instant::now();

    resume.set_position(start_position);
    sync_resume_audio(&mut resume, &audio_tracks, selected_audio);
    sync_resume_subtitle(&mut resume, &path, &subtitle_tracks, selected_subtitle);

    let stdout = io::stdout();
    let mut out =
        BufWriter::with_capacity(canvas.frame_len() + canvas.frame_len() / 2, stdout.lock());
    let mut sequence = Vec::with_capacity(canvas.frame_len() + canvas.frame_len() / 2 + 4096);
    let mut overlay = PlaybackOverlay::new(font_system);
    let mut subtitle_renderer = SubtitleRenderer::new(
        font_system,
        active_subtitle_track(&subtitle_tracks, selected_subtitle)
            .and_then(SubtitleTrack::language),
    );
    clear_screen_and_images(&mut out)?;

    let mut frame = vec![0_u8; target.frame_len()];
    let mut composited_frame = vec![0_u8; canvas.frame_len()];
    let mut previous_image_id = None;
    let mut frame_serial = 0_u32;
    let mut have_frame = false;
    let mut redraw_current_frame = false;
    let mut video_ended = false;
    let frame_interval = frame_interval(source.fps);
    let mut next_frame_at = playback_started_at;
    let mut playback_position = start_position;
    let mut paused = false;
    let mut overlay_visible_until = None::<Instant>;
    let mut last_drawn_overlay_visible = false;
    let mut last_drawn_status_visible = false;
    let mut last_drawn_media_info_visible = false;
    let mut last_drawn_media_info_fps_visible = false;
    let mut status_message = if restored_external_subtitle_missing {
        Some(StatusMessage {
            text: "SAVED SUBTITLE MISSING",
            visible_until: playback_started_at + STATUS_VISIBLE_FOR,
        })
    } else {
        resume.take_error().map(|_| StatusMessage {
            text: "RESUME STATE UNAVAILABLE",
            visible_until: playback_started_at + STATUS_VISIBLE_FOR,
        })
    };
    let mut scrub_position = None::<Duration>;
    let mut keyboard_seek_commit_at = None::<Instant>;
    let mut mouse_scrub_commit_at = None::<Instant>;
    let mut pending_seek = Some(PendingSeek {
        video_generation: decoder.seek_generation(),
        video_target: start_position,
        video_pts: None,
        video_frame_displayed: false,
        audio_generation: audio.as_ref().map(AudioPlayer::seek_generation),
        audio_target: audio.as_ref().map(|_| start_position),
        release_requested: true,
    });
    let mut pending_resize = None::<PendingResize>;
    let mut playback_outcome = PlaybackOutcome::Interrupted;

    loop {
        if shutdown::requested() {
            break;
        }
        resume.maybe_checkpoint(Instant::now());
        if resume.take_error().is_some() {
            status_message = Some(StatusMessage {
                text: "RESUME SAVE FAILED",
                visible_until: Instant::now() + STATUS_VISIBLE_FOR,
            });
        }
        poll_audio(&mut audio, &mut audio_done)?;
        decoder.set_audio_clock(audio.as_ref());
        if progress_pending_seek(&mut pending_seek, &decoder, &mut audio, paused) {
            next_frame_at = Instant::now();
        }

        let input = read_input_events()?;
        let input_at = Instant::now();
        while let Ok(loaded) = embedded_subtitle_rx.try_recv() {
            let loaded_index = loaded.index;
            let loaded_ok = loaded.track.is_some();
            if let Some(slot) = subtitle_tracks.get_mut(loaded_index) {
                slot.track = loaded.track;
            }
            if selected_subtitle == Some(loaded_index) {
                subtitle_renderer = SubtitleRenderer::new(
                    font_system,
                    active_subtitle_track(&subtitle_tracks, selected_subtitle)
                        .and_then(SubtitleTrack::language),
                );
                if !loaded_ok {
                    status_message = Some(StatusMessage {
                        text: "SUBTITLE LOAD FAILED",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                    overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                }
                redraw_current_frame = have_frame;
            }
        }
        let was_overlay_visible = overlay_visible(
            paused,
            scrub_position.is_some(),
            overlay_visible_until,
            input_at,
        );
        if input.mouse_activity {
            overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
            if have_frame && !was_overlay_visible {
                redraw_current_frame = true;
            }
        }

        if let Some(text) = input.text.as_deref() {
            match subtitle_path_from_drop_text(text) {
                Ok(Some(subtitle_path)) => {
                    let key = normalized_subtitle_path(&subtitle_path);
                    if let Some(index) = external_subtitle_paths
                        .iter()
                        .find_map(|(loaded_path, index)| (loaded_path == &key).then_some(*index))
                    {
                        selected_subtitle = Some(index);
                        sync_resume_subtitle(
                            &mut resume,
                            &path,
                            &subtitle_tracks,
                            selected_subtitle,
                        );
                        subtitle_picker_open = false;
                        subtitle_renderer = SubtitleRenderer::new(
                            font_system,
                            active_subtitle_track(&subtitle_tracks, selected_subtitle)
                                .and_then(SubtitleTrack::language),
                        );
                        status_message = Some(StatusMessage {
                            text: "SUBTITLES ALREADY LOADED",
                            visible_until: input_at + STATUS_VISIBLE_FOR,
                        });
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        redraw_current_frame = have_frame;
                    } else {
                        match load_dropped_subtitle_track(&subtitle_path) {
                            Ok(track) => {
                                let index = subtitle_tracks.len();
                                subtitle_labels
                                    .push(Box::leak(track.label().to_string().into_boxed_str()));
                                subtitle_tracks.push(PlaybackSubtitleTrack::loaded_external(
                                    normalized_subtitle_path(&subtitle_path),
                                    track,
                                ));
                                external_subtitle_paths.push((key, index));
                                selected_subtitle = Some(index);
                                sync_resume_subtitle(
                                    &mut resume,
                                    &path,
                                    &subtitle_tracks,
                                    selected_subtitle,
                                );
                                subtitle_picker_open = false;
                                subtitle_renderer = SubtitleRenderer::new(
                                    font_system,
                                    active_subtitle_track(&subtitle_tracks, selected_subtitle)
                                        .and_then(SubtitleTrack::language),
                                );
                                status_message = Some(StatusMessage {
                                    text: "SUBTITLES LOADED",
                                    visible_until: input_at + STATUS_VISIBLE_FOR,
                                });
                                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                                redraw_current_frame = have_frame;
                            }
                            Err(_) => {
                                status_message = Some(StatusMessage {
                                    text: "SUBTITLE LOAD FAILED",
                                    visible_until: input_at + STATUS_VISIBLE_FOR,
                                });
                                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                                redraw_current_frame = have_frame;
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    status_message = Some(StatusMessage {
                        text: "SUBTITLE LOAD FAILED",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                    overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                    redraw_current_frame = have_frame;
                }
            }
        }

        match input.command {
            PlaybackCommand::Quit => {
                playback_outcome = PlaybackOutcome::Quit;
                break;
            }
            PlaybackCommand::TogglePause => {
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                toggle_pause(
                    &mut paused,
                    &decoder,
                    &mut audio,
                    &mut next_frame_at,
                    pending_seek.is_some(),
                );
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::ToggleMute => {
                muted = !muted;
                if let Some(audio) = audio.as_mut() {
                    audio.set_muted(muted);
                }
                status_message = Some(StatusMessage {
                    text: if muted { "MUTE ON" } else { "MUTE OFF" },
                    visible_until: input_at + STATUS_VISIBLE_FOR,
                });
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::ToggleSubtitles => {
                subtitle_picker_open = false;
                if subtitle_tracks.is_empty() {
                    status_message = Some(StatusMessage {
                        text: "NO SUBTITLES",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                } else if selected_subtitle.is_some() {
                    selected_subtitle = None;
                    sync_resume_subtitle(&mut resume, &path, &subtitle_tracks, selected_subtitle);
                    status_message = Some(StatusMessage {
                        text: "SUBTITLES OFF",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                } else {
                    selected_subtitle = Some(0);
                    sync_resume_subtitle(&mut resume, &path, &subtitle_tracks, selected_subtitle);
                    subtitle_renderer = SubtitleRenderer::new(
                        font_system,
                        active_subtitle_track(&subtitle_tracks, selected_subtitle)
                            .and_then(SubtitleTrack::language),
                    );
                    status_message = Some(StatusMessage {
                        text: "SUBTITLES ON",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                }
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::ShowMediaInfo => {
                media_info.show(input_at);
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::ToggleMediaInfo => {
                media_info.toggle();
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::SeekBySeconds(seconds) => {
                let base_position = scrub_position.unwrap_or(playback_position);
                let seek_target = seek_position(base_position, seconds, source.duration);
                if is_end_seek(seek_target, source.duration) {
                    playback_outcome = PlaybackOutcome::Completed;
                    break;
                }
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                if keyboard_seek_commit_at.is_none_or(|deadline| input_at >= deadline) {
                    let mut seek = seek_playback(
                        &path,
                        source.has_audio,
                        &mut decoder,
                        &mut audio,
                        &mut audio_done,
                        selected_audio_stream_choice(&audio_tracks, selected_audio),
                        seek_target,
                        true,
                        paused,
                        muted,
                    )?;
                    seek.hold();
                    pending_seek = Some(seek);
                    video_ended = false;
                    next_frame_at = Instant::now();
                    redraw_current_frame = false;
                } else {
                    redraw_current_frame = have_frame;
                }
                scrub_position = Some(seek_target);
                keyboard_seek_commit_at = Some(input_at + KEYBOARD_SEEK_COMMIT_AFTER);
            }
            PlaybackCommand::None => {}
        }

        if advance_keyboard_seek_preview(
            &mut pending_seek,
            &mut decoder,
            scrub_position,
            keyboard_seek_commit_at.is_some(),
        ) {
            next_frame_at = Instant::now();
            redraw_current_frame = false;
        }

        let (observed_target, observed_canvas) =
            terminal_target_and_canvas(source.width, source.height);
        let resize_ready = settled_resize_layout(
            target,
            canvas,
            observed_target,
            observed_canvas,
            &mut pending_resize,
            Instant::now(),
        );
        let (current_target, current_canvas) = resize_ready.unwrap_or((target, canvas));
        if pending_resize.is_some() && resize_ready.is_none() {
            out.flush()?;
            thread::sleep(Duration::from_millis(8));
            resume.maybe_checkpoint(Instant::now());
            continue;
        }

        if current_target != target {
            let resize_position =
                resize_restart_position(playback_position, source.duration, paused, audio.as_ref());

            decoder.stop()?;
            frame.resize(current_target.frame_len(), 0);
            target = current_target;
            decoder = VideoDecoder::spawn_at(
                &path,
                target.width,
                target.height,
                source.fps,
                resize_position,
                true,
            )?;
            decoder.set_audio_clock(audio.as_ref());
            pending_seek = Some(PendingSeek {
                video_generation: decoder.seek_generation(),
                video_target: resize_position,
                video_pts: None,
                video_frame_displayed: false,
                audio_generation: None,
                audio_target: None,
                release_requested: true,
            });
            playback_position = resize_position;
            resume.set_position(playback_position);

            clear_screen_and_images(&mut out)?;
            previous_image_id = None;
            have_frame = false;
            redraw_current_frame = false;
            last_drawn_overlay_visible = false;
            last_drawn_status_visible = false;
            last_drawn_media_info_visible = false;
            last_drawn_media_info_fps_visible = false;
            scrub_position = None;
            keyboard_seek_commit_at = None;
            video_ended = false;
            next_frame_at = Instant::now();
        }

        if current_canvas != canvas {
            canvas = current_canvas;
            composited_frame.resize(canvas.frame_len(), 0);
            clear_screen_and_images(&mut out)?;
            previous_image_id = None;
            last_drawn_overlay_visible = false;
            last_drawn_status_visible = false;
            last_drawn_media_info_visible = false;
            last_drawn_media_info_fps_visible = false;
            if have_frame {
                let state = overlay_state(
                    playback_position,
                    scrub_position,
                    source.duration,
                    paused,
                    overlay_visible_until,
                    status_message,
                    !audio_tracks.is_empty(),
                    selected_audio,
                    audio_picker_open,
                    audio_labels.clone(),
                    !subtitle_tracks.is_empty(),
                    selected_subtitle,
                    subtitle_picker_open,
                    subtitle_labels.clone(),
                    media_title,
                    media_info.state(selected_audio, canvas, &decoder, paused, Instant::now()),
                );
                draw_frame(
                    &mut out,
                    target,
                    canvas,
                    &mut previous_image_id,
                    &mut frame_serial,
                    &frame,
                    &mut composited_frame,
                    &mut subtitle_renderer,
                    active_subtitle_track(&subtitle_tracks, selected_subtitle),
                    selected_subtitle.is_some(),
                    playback_position,
                    &mut overlay,
                    &state,
                    &mut sequence,
                )?;
                last_drawn_overlay_visible = state.visible;
                last_drawn_status_visible = state.status_message.is_some();
                last_drawn_media_info_visible = state.media_info.is_some();
                last_drawn_media_info_fps_visible = media_info_fps_visible(&state);
                redraw_current_frame = false;
            }
        }

        let mut pointer_seek_target = None;
        let hit_context = OverlayHitContext {
            width: canvas.width,
            height: canvas.height,
            terminal_rows: canvas.area.rows,
            scale_percent: canvas.overlay_scale_percent,
            position: scrub_position.unwrap_or(playback_position),
            duration: source.duration,
            audio_available: !audio_tracks.is_empty(),
            audio_count: audio_tracks.len(),
            subtitles_available: !subtitle_tracks.is_empty(),
            subtitle_count: subtitle_tracks.len(),
        };
        if !input.mouse_events.is_empty()
            && keyboard_seek_commit_at.take().is_some()
            && let (Some(seek), Some(seek_target)) = (pending_seek.as_mut(), scrub_position.take())
        {
            if seek.needs_exact_retarget_for_release(seek_target) {
                seek.retarget_video(&mut decoder, seek_target, true);
            }
            seek.request_release();
            next_frame_at = Instant::now();
        }
        for mouse in input.mouse_events {
            let seek_target = match mouse {
                PlaybackMouse::Down { column, row } => {
                    let point = mouse_canvas_position(column, row, canvas);
                    if let Some(action) = point.and_then(|point| {
                        overlay.audio_picker_action(hit_context, point, audio_picker_open)
                    }) {
                        scrub_position = None;
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        match action {
                            AudioPickerAction::TogglePicker => {
                                audio_picker_open = !audio_picker_open;
                                if audio_picker_open {
                                    subtitle_picker_open = false;
                                }
                            }
                            AudioPickerAction::SelectTrack(index) => {
                                selected_audio = Some(index);
                                sync_resume_audio(&mut resume, &audio_tracks, selected_audio);
                                audio_picker_open = false;
                                if let Some(mut player) = audio.take() {
                                    player.stop()?;
                                }
                                audio_done = true;
                                pending_seek = Some(seek_playback(
                                    &path,
                                    source.has_audio,
                                    &mut decoder,
                                    &mut audio,
                                    &mut audio_done,
                                    selected_audio_stream_choice(&audio_tracks, selected_audio),
                                    playback_position,
                                    true,
                                    paused,
                                    muted,
                                )?);
                            }
                        }
                        redraw_current_frame = have_frame;
                    } else if let Some(action) = point.and_then(|point| {
                        overlay.subtitle_picker_action(hit_context, point, subtitle_picker_open)
                    }) {
                        scrub_position = None;
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        match action {
                            SubtitlePickerAction::TogglePicker => {
                                subtitle_picker_open = !subtitle_picker_open;
                                if subtitle_picker_open {
                                    audio_picker_open = false;
                                }
                            }
                            SubtitlePickerAction::SelectTrack(index) => {
                                selected_subtitle = Some(index);
                                sync_resume_subtitle(
                                    &mut resume,
                                    &path,
                                    &subtitle_tracks,
                                    selected_subtitle,
                                );
                                subtitle_picker_open = false;
                                subtitle_renderer = SubtitleRenderer::new(
                                    font_system,
                                    active_subtitle_track(&subtitle_tracks, selected_subtitle)
                                        .and_then(SubtitleTrack::language),
                                );
                                if active_subtitle_track(&subtitle_tracks, selected_subtitle)
                                    .is_none()
                                {
                                    status_message = Some(StatusMessage {
                                        text: "SUBTITLE LOADING",
                                        visible_until: input_at + STATUS_VISIBLE_FOR,
                                    });
                                    overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                                }
                            }
                            SubtitlePickerAction::SelectOff => {
                                selected_subtitle = None;
                                sync_resume_subtitle(
                                    &mut resume,
                                    &path,
                                    &subtitle_tracks,
                                    selected_subtitle,
                                );
                                subtitle_picker_open = false;
                            }
                        }
                        redraw_current_frame = have_frame;
                    } else if point
                        .is_some_and(|point| overlay.playback_button_hit_test(hit_context, point))
                    {
                        scrub_position = None;
                        subtitle_picker_open = false;
                        audio_picker_open = false;
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        toggle_pause(
                            &mut paused,
                            &decoder,
                            &mut audio,
                            &mut next_frame_at,
                            pending_seek.is_some(),
                        );
                        redraw_current_frame = have_frame;
                    } else {
                        let picker_was_open = audio_picker_open || subtitle_picker_open;
                        audio_picker_open = false;
                        subtitle_picker_open = false;
                        scrub_position = point
                            .and_then(|point| overlay.progress_hit_test(hit_context, point))
                            .and_then(|ratio| seek_from_progress_ratio(ratio, source.duration));
                        mouse_scrub_commit_at =
                            scrub_position.map(|_| input_at + MOUSE_SCRUB_COMMIT_AFTER);
                        if picker_was_open || scrub_position.is_some() {
                            redraw_current_frame = have_frame;
                        }
                    }
                    None
                }
                PlaybackMouse::Drag { column, row } if scrub_position.is_some() => {
                    let x = mouse_canvas_x(column, row, canvas);
                    let ratio = overlay.progress_ratio_from_x(
                        canvas.width,
                        canvas.height,
                        canvas.area.rows,
                        canvas.overlay_scale_percent,
                        source.duration,
                        !subtitle_tracks.is_empty(),
                        !audio_tracks.is_empty(),
                        x,
                    );
                    scrub_position = seek_from_progress_ratio(ratio, source.duration);
                    redraw_current_frame = have_frame;
                    if mouse_scrub_commit_at.is_some_and(|deadline| input_at >= deadline) {
                        mouse_scrub_commit_at = Some(input_at + MOUSE_SCRUB_COMMIT_AFTER);
                        scrub_position
                    } else {
                        None
                    }
                }
                PlaybackMouse::Up { column, row } if scrub_position.is_some() => {
                    let x = mouse_canvas_x(column, row, canvas);
                    let ratio = overlay.progress_ratio_from_x(
                        canvas.width,
                        canvas.height,
                        canvas.area.rows,
                        canvas.overlay_scale_percent,
                        source.duration,
                        !subtitle_tracks.is_empty(),
                        !audio_tracks.is_empty(),
                        x,
                    );
                    let target = seek_from_progress_ratio(ratio, source.duration);
                    scrub_position = None;
                    mouse_scrub_commit_at = None;
                    target
                }
                PlaybackMouse::Up { .. } => {
                    scrub_position = None;
                    mouse_scrub_commit_at = None;
                    None
                }
                _ => None,
            };

            if let Some(seek_target) = seek_target {
                pointer_seek_target = Some(seek_target);
            }
        }

        if let Some(seek_target) = pointer_seek_target {
            keyboard_seek_commit_at = None;
            if is_end_seek(seek_target, source.duration) {
                playback_outcome = PlaybackOutcome::Completed;
                break;
            }
            pending_seek = Some(seek_playback(
                &path,
                source.has_audio,
                &mut decoder,
                &mut audio,
                &mut audio_done,
                selected_audio_stream_choice(&audio_tracks, selected_audio),
                seek_target,
                true,
                paused,
                muted,
            )?);
            playback_position = seek_target;
            resume.set_position(playback_position);
            video_ended = false;
            next_frame_at = Instant::now();
            redraw_current_frame = false;
        }

        if keyboard_seek_commit_at.is_some_and(|deadline| Instant::now() >= deadline) {
            keyboard_seek_commit_at = None;
            if let Some(seek_target) = scrub_position.take() {
                if let Some(seek) = pending_seek.as_mut() {
                    if seek.needs_exact_retarget_for_release(seek_target) {
                        seek.retarget_video(&mut decoder, seek_target, true);
                    }
                    seek.request_release();
                } else {
                    pending_seek = Some(seek_playback(
                        &path,
                        source.has_audio,
                        &mut decoder,
                        &mut audio,
                        &mut audio_done,
                        selected_audio_stream_choice(&audio_tracks, selected_audio),
                        seek_target,
                        true,
                        paused,
                        muted,
                    )?);
                }
                video_ended = false;
                next_frame_at = Instant::now();
                redraw_current_frame = false;
            }
        }

        let overlay_is_visible = overlay_visible(
            paused,
            scrub_position.is_some(),
            overlay_visible_until,
            Instant::now(),
        );
        let status_is_visible = status_text(status_message, Instant::now()).is_some();
        let media_info_is_visible = media_info.visible(Instant::now());
        let media_info_fps_is_visible = media_info_is_visible
            && media_info_display_fps(paused, decoder.display_fps(Instant::now())).is_some();
        if have_frame
            && ((last_drawn_overlay_visible && !overlay_is_visible)
                || (last_drawn_status_visible && !status_is_visible)
                || (last_drawn_media_info_visible && !media_info_is_visible)
                || (last_drawn_media_info_fps_visible && !media_info_fps_is_visible))
        {
            redraw_current_frame = true;
        }

        if redraw_current_frame && have_frame {
            let state = overlay_state(
                playback_position,
                scrub_position,
                source.duration,
                paused,
                overlay_visible_until,
                status_message,
                !audio_tracks.is_empty(),
                selected_audio,
                audio_picker_open,
                audio_labels.clone(),
                !subtitle_tracks.is_empty(),
                selected_subtitle,
                subtitle_picker_open,
                subtitle_labels.clone(),
                media_title,
                media_info.state(selected_audio, canvas, &decoder, paused, Instant::now()),
            );
            draw_frame(
                &mut out,
                target,
                canvas,
                &mut previous_image_id,
                &mut frame_serial,
                &frame,
                &mut composited_frame,
                &mut subtitle_renderer,
                active_subtitle_track(&subtitle_tracks, selected_subtitle),
                selected_subtitle.is_some(),
                playback_position,
                &mut overlay,
                &state,
                &mut sequence,
            )?;
            last_drawn_overlay_visible = state.visible;
            last_drawn_status_visible = state.status_message.is_some();
            last_drawn_media_info_visible = state.media_info.is_some();
            last_drawn_media_info_fps_visible = media_info_fps_visible(&state);
            redraw_current_frame = false;
            out.flush()?;
        }

        if paused {
            match decoder.read_latest_frame(&mut frame)? {
                FrameStatus::NewFrame { pts } => {
                    playback_position = pts;
                    resume.set_position(playback_position);
                    let state = overlay_state(
                        playback_position,
                        scrub_position,
                        source.duration,
                        paused,
                        overlay_visible_until,
                        status_message,
                        !audio_tracks.is_empty(),
                        selected_audio,
                        audio_picker_open,
                        audio_labels.clone(),
                        !subtitle_tracks.is_empty(),
                        selected_subtitle,
                        subtitle_picker_open,
                        subtitle_labels.clone(),
                        media_title,
                        media_info.state(selected_audio, canvas, &decoder, paused, Instant::now()),
                    );
                    draw_frame(
                        &mut out,
                        target,
                        canvas,
                        &mut previous_image_id,
                        &mut frame_serial,
                        &frame,
                        &mut composited_frame,
                        &mut subtitle_renderer,
                        active_subtitle_track(&subtitle_tracks, selected_subtitle),
                        selected_subtitle.is_some(),
                        playback_position,
                        &mut overlay,
                        &state,
                        &mut sequence,
                    )?;
                    have_frame = true;
                    last_drawn_overlay_visible = state.visible;
                    last_drawn_status_visible = state.status_message.is_some();
                    last_drawn_media_info_visible = state.media_info.is_some();
                    last_drawn_media_info_fps_visible = media_info_fps_visible(&state);
                    redraw_current_frame = false;
                    mark_pending_seek_frame_displayed(&mut pending_seek, pts);
                    advance_keyboard_seek_preview(
                        &mut pending_seek,
                        &mut decoder,
                        scrub_position,
                        keyboard_seek_commit_at.is_some(),
                    );
                }
                FrameStatus::NoFrame => {}
                FrameStatus::Ended => {
                    video_ended = true;
                }
            }
            out.flush()?;
            thread::sleep(Duration::from_millis(15));
            resume.maybe_checkpoint(Instant::now());
            continue;
        }

        let now = Instant::now();
        if now < next_frame_at {
            out.flush()?;
            thread::sleep((next_frame_at - now).min(Duration::from_millis(5)));
            resume.maybe_checkpoint(Instant::now());
            continue;
        }

        match decoder.read_latest_frame(&mut frame)? {
            FrameStatus::NewFrame { pts } => {
                playback_position = pts;
                resume.set_position(playback_position);
                let state = overlay_state(
                    playback_position,
                    scrub_position,
                    source.duration,
                    paused,
                    overlay_visible_until,
                    status_message,
                    !audio_tracks.is_empty(),
                    selected_audio,
                    audio_picker_open,
                    audio_labels.clone(),
                    !subtitle_tracks.is_empty(),
                    selected_subtitle,
                    subtitle_picker_open,
                    subtitle_labels.clone(),
                    media_title,
                    media_info.state(selected_audio, canvas, &decoder, paused, Instant::now()),
                );
                draw_frame(
                    &mut out,
                    target,
                    canvas,
                    &mut previous_image_id,
                    &mut frame_serial,
                    &frame,
                    &mut composited_frame,
                    &mut subtitle_renderer,
                    active_subtitle_track(&subtitle_tracks, selected_subtitle),
                    selected_subtitle.is_some(),
                    playback_position,
                    &mut overlay,
                    &state,
                    &mut sequence,
                )?;
                have_frame = true;
                last_drawn_overlay_visible = state.visible;
                last_drawn_status_visible = state.status_message.is_some();
                last_drawn_media_info_visible = state.media_info.is_some();
                last_drawn_media_info_fps_visible = media_info_fps_visible(&state);
                redraw_current_frame = false;
                out.flush()?;
                mark_pending_seek_frame_displayed(&mut pending_seek, pts);
                if advance_keyboard_seek_preview(
                    &mut pending_seek,
                    &mut decoder,
                    scrub_position,
                    keyboard_seek_commit_at.is_some(),
                ) {
                    next_frame_at = Instant::now();
                }
                advance_frame_clock(&mut next_frame_at, frame_interval);
            }
            FrameStatus::NoFrame => {
                out.flush()?;
                thread::sleep(Duration::from_millis(2));
            }
            FrameStatus::Ended => {
                video_ended = true;
                if let Some(duration) = source.duration {
                    playback_position = duration;
                    resume.set_position(playback_position);
                }
                if have_frame {
                    let state = overlay_state(
                        playback_position,
                        scrub_position,
                        source.duration,
                        paused,
                        overlay_visible_until,
                        status_message,
                        !audio_tracks.is_empty(),
                        selected_audio,
                        audio_picker_open,
                        audio_labels.clone(),
                        !subtitle_tracks.is_empty(),
                        selected_subtitle,
                        subtitle_picker_open,
                        subtitle_labels.clone(),
                        media_title,
                        media_info.state(selected_audio, canvas, &decoder, paused, Instant::now()),
                    );
                    draw_frame(
                        &mut out,
                        target,
                        canvas,
                        &mut previous_image_id,
                        &mut frame_serial,
                        &frame,
                        &mut composited_frame,
                        &mut subtitle_renderer,
                        active_subtitle_track(&subtitle_tracks, selected_subtitle),
                        selected_subtitle.is_some(),
                        playback_position,
                        &mut overlay,
                        &state,
                        &mut sequence,
                    )?;
                    last_drawn_overlay_visible = state.visible;
                    last_drawn_status_visible = state.status_message.is_some();
                    last_drawn_media_info_visible = state.media_info.is_some();
                    last_drawn_media_info_fps_visible = media_info_fps_visible(&state);
                    redraw_current_frame = false;
                }
                out.flush()?;
                thread::sleep(Duration::from_millis(10));
            }
        }

        if video_ended && audio_done {
            playback_outcome = PlaybackOutcome::Completed;
            break;
        }
    }

    let resume_result = match playback_outcome {
        PlaybackOutcome::Completed => resume.clear(),
        PlaybackOutcome::Quit | PlaybackOutcome::Interrupted => resume.save_now(),
    };

    let decoder_result = decoder.stop();
    let audio_result = audio.as_mut().map(AudioPlayer::stop).transpose();
    resume_result.context("failed to persist playback state")?;
    decoder_result?;
    audio_result?;
    Ok(())
}

fn settled_resize_layout(
    active_target: TargetFrame,
    active_canvas: CanvasFrame,
    observed_target: TargetFrame,
    observed_canvas: CanvasFrame,
    pending: &mut Option<PendingResize>,
    now: Instant,
) -> Option<(TargetFrame, CanvasFrame)> {
    if observed_target == active_target && observed_canvas == active_canvas {
        *pending = None;
        return None;
    }

    match pending {
        Some(resize) if resize.target == observed_target && resize.canvas == observed_canvas => {
            if now.duration_since(resize.observed_at) >= RESIZE_SETTLE_FOR {
                *pending = None;
                Some((observed_target, observed_canvas))
            } else {
                None
            }
        }
        _ => {
            *pending = Some(PendingResize {
                target: observed_target,
                canvas: observed_canvas,
                observed_at: now,
            });
            None
        }
    }
}

fn resize_restart_position(
    playback_position: Duration,
    duration: Option<Duration>,
    paused: bool,
    audio: Option<&AudioPlayer>,
) -> Duration {
    let position = if paused {
        playback_position
    } else {
        audio
            .and_then(AudioPlayer::playback_position)
            .unwrap_or(playback_position)
    };
    duration.map_or(position, |duration| position.min(duration))
}

#[allow(clippy::too_many_arguments)]
fn draw_frame(
    out: &mut impl Write,
    target: TargetFrame,
    canvas: CanvasFrame,
    previous_image_id: &mut Option<u32>,
    frame_serial: &mut u32,
    frame: &[u8],
    composited_frame: &mut [u8],
    subtitle_renderer: &mut SubtitleRenderer,
    subtitle_track: Option<&SubtitleTrack>,
    subtitles_visible: bool,
    playback_position: Duration,
    overlay: &mut PlaybackOverlay,
    overlay_state: &OverlayState,
    sequence: &mut Vec<u8>,
) -> io::Result<()> {
    if frame.len() != target.frame_len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "decoded frame length does not match target frame length",
        ));
    }
    if composited_frame.len() != canvas.frame_len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "composited frame length does not match canvas frame length",
        ));
    }

    composited_frame.fill(0);
    copy_video_into_canvas(frame, target, composited_frame, canvas);
    if subtitles_visible && let Some(subtitle_track) = subtitle_track {
        subtitle_renderer.render(
            composited_frame,
            canvas.width,
            canvas.height,
            subtitle_track,
            playback_position,
            subtitle_bottom_reserve(canvas.height, overlay_state.visible),
        );
    }
    overlay.render(
        composited_frame,
        canvas.width,
        canvas.height,
        canvas.area.rows,
        canvas.overlay_scale_percent,
        overlay_state.clone(),
    );

    let image_id = KITTY_IMAGE_IDS[(*frame_serial as usize) % KITTY_IMAGE_IDS.len()];
    write_kitty_rgb_frame(
        out,
        KittyFramePlacement {
            image_id,
            placement_id: KITTY_PLACEMENT_ID,
            z_index: 0,
            previous_image_id: *previous_image_id,
            width: canvas.width,
            height: canvas.height,
            area: canvas.area,
        },
        composited_frame,
        sequence,
    )?;
    *previous_image_id = Some(image_id);
    *frame_serial = frame_serial.wrapping_add(1);
    Ok(())
}

fn copy_video_into_canvas(
    frame: &[u8],
    target: TargetFrame,
    canvas_frame: &mut [u8],
    canvas: CanvasFrame,
) {
    let dst_width = canvas.width as usize;
    let dst_x = canvas.video_x as usize;
    let dst_y = canvas.video_y as usize;
    let video_width = canvas
        .video_width
        .min(canvas.width.saturating_sub(canvas.video_x)) as usize;
    let video_height = canvas
        .video_height
        .min(canvas.height.saturating_sub(canvas.video_y)) as usize;
    if video_width == 0 || video_height == 0 {
        return;
    }

    let src_width = target.width as usize;
    let src_height = target.height as usize;
    let src_row_bytes = src_width * 3;
    if video_width == src_width && video_height == src_height {
        for row in 0..src_height {
            let src_start = row * src_row_bytes;
            let dst_start = ((dst_y + row) * dst_width + dst_x) * 3;
            canvas_frame[dst_start..dst_start + src_row_bytes]
                .copy_from_slice(&frame[src_start..src_start + src_row_bytes]);
        }
        return;
    }

    for dst_row in 0..video_height {
        let src_row = (dst_row * src_height / video_height).min(src_height.saturating_sub(1));
        let src_row_start = src_row * src_row_bytes;
        let dst_row_start = ((dst_y + dst_row) * dst_width + dst_x) * 3;
        for dst_col in 0..video_width {
            let src_col = (dst_col * src_width / video_width).min(src_width.saturating_sub(1));
            let src_offset = src_row_start + src_col * 3;
            let dst_offset = dst_row_start + dst_col * 3;
            canvas_frame[dst_offset..dst_offset + 3]
                .copy_from_slice(&frame[src_offset..src_offset + 3]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn seek_playback(
    path: &Path,
    has_audio: bool,
    decoder: &mut VideoDecoder,
    audio: &mut Option<AudioPlayer>,
    audio_done: &mut bool,
    audio_stream: Option<Option<usize>>,
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
    if has_audio && let Some(audio_stream_index) = audio_stream {
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

fn progress_pending_seek(
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

fn mark_pending_seek_frame_displayed(pending: &mut Option<PendingSeek>, pts: Duration) {
    if let Some(seek) = pending.as_mut() {
        seek.mark_video_frame_displayed(pts);
    }
}

fn keyboard_preview_target(
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

fn advance_keyboard_seek_preview(
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

fn toggle_pause(
    paused: &mut bool,
    decoder: &VideoDecoder,
    audio: &mut Option<AudioPlayer>,
    next_frame_at: &mut Instant,
    seek_pending: bool,
) {
    *paused = !*paused;
    decoder.set_paused(*paused || seek_pending);
    if let Some(audio) = audio.as_mut() {
        audio.set_paused(*paused);
    }
    if !*paused {
        *next_frame_at = Instant::now();
    }
}

#[allow(clippy::too_many_arguments)]
fn overlay_state(
    position: Duration,
    scrub_position: Option<Duration>,
    duration: Option<Duration>,
    paused: bool,
    visible_until: Option<Instant>,
    status_message: Option<StatusMessage>,
    audio_available: bool,
    selected_audio: Option<usize>,
    audio_picker_open: bool,
    audio_labels: Vec<&'static str>,
    subtitles_available: bool,
    selected_subtitle: Option<usize>,
    subtitle_picker_open: bool,
    subtitle_labels: Vec<&'static str>,
    media_title: &'static str,
    media_info: Option<MediaInfoState>,
) -> OverlayState {
    let now = Instant::now();
    OverlayState {
        position: scrub_position.unwrap_or(position),
        duration,
        paused,
        visible: overlay_visible(paused, scrub_position.is_some(), visible_until, now)
            || audio_picker_open
            || subtitle_picker_open,
        audio_available,
        selected_audio,
        audio_picker_open,
        audio_labels,
        subtitles_available,
        selected_subtitle,
        subtitle_picker_open,
        subtitle_labels,
        status_message: status_text(status_message, now),
        media_title: Some(media_title),
        media_info,
    }
}

fn media_info_fps_visible(state: &OverlayState) -> bool {
    state
        .media_info
        .as_ref()
        .is_some_and(|info| info.display_fps.is_some())
}

fn media_info_display_fps(paused: bool, sampled_fps: Option<f64>) -> Option<f64> {
    (!paused).then_some(sampled_fps).flatten()
}

fn file_info_summary(path: &Path, source: &VideoInfo) -> String {
    let mut parts = Vec::new();
    let path_text = path.to_string_lossy();
    if let Some((scheme, _)) = path_text.split_once("://")
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        parts.push(scheme.to_ascii_uppercase());
    }
    if let Some(container) = source.container.as_deref() {
        parts.push(container_display_name(container));
    }
    if let Ok(metadata) = fs::metadata(path)
        && metadata.is_file()
    {
        parts.push(format_file_size(metadata.len()));
    }
    if parts.is_empty() {
        "Unknown".to_string()
    } else {
        parts.join(" · ")
    }
}

fn container_display_name(container: &str) -> String {
    match container.split(',').next().unwrap_or(container) {
        "matroska" => "Matroska".to_string(),
        "mov" => "MP4 / MOV".to_string(),
        "mpegts" => "MPEG-TS".to_string(),
        "mpeg" => "MPEG".to_string(),
        "avi" => "AVI".to_string(),
        "flv" => "FLV".to_string(),
        "ogg" => "Ogg".to_string(),
        "hls" => "HLS".to_string(),
        value => value.to_ascii_uppercase(),
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn media_title(path: &Path) -> &'static str {
    let text = path
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Box::leak(text.into_boxed_str())
}

fn status_text(message: Option<StatusMessage>, now: Instant) -> Option<&'static str> {
    message.and_then(|message| (now < message.visible_until).then_some(message.text))
}

fn subtitle_bottom_reserve(height: u32, overlay_visible: bool) -> u32 {
    if overlay_visible {
        (height / 7).clamp(28, 64)
    } else {
        0
    }
}

fn overlay_visible(
    paused: bool,
    scrubbing: bool,
    visible_until: Option<Instant>,
    now: Instant,
) -> bool {
    paused || scrubbing || visible_until.is_some_and(|until| now < until)
}

fn seek_from_progress_ratio(ratio: f64, duration: Option<Duration>) -> Option<Duration> {
    duration.map(|duration| Duration::from_secs_f64(duration.as_secs_f64() * ratio.clamp(0.0, 1.0)))
}

fn mouse_canvas_position(column: u16, row: u16, canvas: CanvasFrame) -> Option<OverlayHitPoint> {
    if mouse_coordinates_are_pixels(column, row, canvas) {
        let x = pixel_to_canvas(u32::from(column), canvas.terminal_width, canvas.width);
        let y = pixel_to_canvas(u32::from(row), canvas.terminal_height, canvas.height);
        return Some(OverlayHitPoint {
            x,
            y,
            cell: HitboxRect {
                left: x,
                top: y,
                right: x,
                bottom: y,
            },
        });
    }

    let end_col = canvas.area.x.saturating_add(canvas.area.cols);
    let end_row = canvas.area.y.saturating_add(canvas.area.rows);
    if column < canvas.area.x || column >= end_col || row < canvas.area.y || row >= end_row {
        return None;
    }

    let rel_col = column - canvas.area.x;
    let rel_row = row - canvas.area.y;
    let x = cell_to_pixel(rel_col, canvas.area.cols, canvas.width);
    let y = cell_to_pixel(rel_row, canvas.area.rows, canvas.height);

    Some(OverlayHitPoint {
        x,
        y,
        cell: HitboxRect {
            left: x,
            top: y,
            right: x,
            bottom: y,
        },
    })
}

fn mouse_canvas_x(column: u16, row: u16, canvas: CanvasFrame) -> u32 {
    if mouse_coordinates_are_pixels(column, row, canvas) {
        return pixel_to_canvas(u32::from(column), canvas.terminal_width, canvas.width);
    }

    let rel = if column <= canvas.area.x {
        0
    } else {
        column
            .saturating_sub(canvas.area.x)
            .min(canvas.area.cols.saturating_sub(1))
    };
    cell_to_pixel(rel, canvas.area.cols, canvas.width)
}

fn mouse_coordinates_are_pixels(column: u16, row: u16, canvas: CanvasFrame) -> bool {
    column >= canvas.area.cols || row >= canvas.area.rows
}

fn cell_to_pixel(cell: u16, cells: u16, pixels: u32) -> u32 {
    let cells = f64::from(cells.max(1));
    let pixels = pixels.max(1);
    (((f64::from(cell) + 0.5) * f64::from(pixels)) / cells)
        .floor()
        .min(f64::from(pixels - 1)) as u32
}

fn pixel_to_canvas(pixel: u32, terminal_pixels: u32, canvas_pixels: u32) -> u32 {
    let terminal_pixels = terminal_pixels.max(1);
    let canvas_pixels = canvas_pixels.max(1);
    (u64::from(pixel.min(terminal_pixels.saturating_sub(1))) * u64::from(canvas_pixels)
        / u64::from(terminal_pixels))
    .min(u64::from(canvas_pixels - 1)) as u32
}

fn frame_interval(fps: f64) -> Duration {
    Duration::from_secs_f64(1.0 / fps.max(1.0))
}

fn advance_frame_clock(next_frame_at: &mut Instant, frame_interval: Duration) {
    *next_frame_at += frame_interval;

    let now = Instant::now();
    if *next_frame_at + frame_interval < now {
        *next_frame_at = now + frame_interval;
    }
}

fn seek_position(current: Duration, seconds: i32, duration: Option<Duration>) -> Duration {
    let delta = Duration::from_secs(seconds.unsigned_abs().into());
    let target = if seconds < 0 {
        current.saturating_sub(delta)
    } else {
        current.checked_add(delta).unwrap_or(Duration::MAX)
    };

    duration.map_or(target, |duration| target.min(duration))
}

fn is_end_seek(target: Duration, duration: Option<Duration>) -> bool {
    duration.is_some_and(|duration| target >= duration)
}

fn poll_audio(audio: &mut Option<AudioPlayer>, audio_done: &mut bool) -> Result<()> {
    if let Some(player) = audio.as_mut()
        && player.is_finished()?
    {
        *audio = None;
        *audio_done = true;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
