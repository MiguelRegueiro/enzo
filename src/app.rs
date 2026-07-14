use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

use crate::{
    drop_target::{draw_drop_target, is_remote_url_text, media_candidates_from_text},
    font_system::FontSystem,
    input::{DropCommand, PlaybackCommand, PlaybackMouse, read_drop_events, read_input_events},
    media::{AudioPlayer, FrameStatus, VideoDecoder, probe_video},
    overlay::{
        HitboxRect, OverlayHitContext, OverlayHitPoint, OverlayState, PlaybackOverlay,
        SubtitlePickerAction,
    },
    subtitle::{
        SubtitleRenderer, SubtitleTrack, load_embedded_subtitle_tracks, sidecar_subtitle_path,
    },
    terminal::{
        ImageArea, KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, TerminalGuard,
        clear_screen_and_images, enable_tmux_passthrough, inside_tmux, looks_like_kitty,
        terminal_pixel_size, write_kitty_rgb_frame,
    },
};

const MAX_DECODE_WIDTH: u32 = 1920;
const MAX_DECODE_HEIGHT: u32 = 1080;
const MAX_CANVAS_WIDTH: u32 = 1920;
const MAX_CANVAS_HEIGHT: u32 = 1200;
const NORMAL_OVERLAY_SCALE_PERCENT: u32 = 100;
const MAX_OVERLAY_SCALE_PERCENT: u32 = 125;
const OVERLAY_VISIBLE_FOR: Duration = Duration::from_secs(2);
const STATUS_VISIBLE_FOR: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct StatusMessage {
    text: &'static str,
    visible_until: Instant,
}

pub(crate) fn run() -> Result<()> {
    let config = parse_args(env::args_os().skip(1))?;
    let font_system = FontSystem::discover();
    if !config.force && !looks_like_kitty() {
        bail!(
            "Rigoberto targets Kitty graphics; run from kitty or pass --force if your terminal is compatible"
        );
    }

    if inside_tmux() {
        enable_tmux_passthrough();
    }

    if let Some(path) = config.path {
        let _terminal = TerminalGuard::enter()?;
        play_media(path, config.sub_file.as_deref(), &font_system)
    } else {
        run_drop_target(config.sub_file.as_deref(), &font_system)
    }
}

fn run_drop_target(sub_file: Option<&Path>, font_system: &FontSystem) -> Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut status = None::<String>;

    loop {
        draw_drop_target(&mut out, status.as_deref())?;
        let input = read_drop_events()?;
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
                return play_media(path, sub_file, font_system);
            }
            Err(error) => {
                status = Some(error.to_string());
            }
        }
    }
}

fn play_media(path: PathBuf, sub_file: Option<&Path>, font_system: &FontSystem) -> Result<()> {
    let source = probe_video(&path)
        .with_context(|| format!("failed to inspect video metadata for {}", path.display()))?;
    let mut subtitle_tracks = load_subtitle_tracks(&path, sub_file)?;
    let mut subtitle_labels = subtitle_tracks
        .iter()
        .map(|track| Box::leak(track.label().to_string().into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    let mut selected_subtitle = (!subtitle_tracks.is_empty()).then_some(0_usize);
    let mut external_subtitle_paths = Vec::<(PathBuf, usize)>::new();
    if let (Some(path), true) = (
        external_subtitle_path(&path, sub_file),
        !subtitle_tracks.is_empty(),
    ) {
        external_subtitle_paths.push((normalized_subtitle_path(&path), 0));
    }
    let mut subtitle_picker_open = false;
    let media_title = media_title(&path);
    let (mut target, mut canvas) = terminal_target_and_canvas(source.width, source.height);

    let mut decoder = VideoDecoder::spawn(&path, target.width, target.height, source.fps)?;
    let mut muted = false;
    let mut audio = if source.has_audio {
        Some(AudioPlayer::spawn(&path, muted)?)
    } else {
        None
    };
    let mut audio_done = !source.has_audio;
    let playback_started_at = Instant::now();

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
    let mut playback_position = Duration::ZERO;
    let mut paused = false;
    let mut overlay_visible_until = None::<Instant>;
    let mut last_drawn_overlay_visible = false;
    let mut last_drawn_status_visible = false;
    let mut status_message = None::<StatusMessage>;
    let mut scrub_position = None::<Duration>;

    loop {
        poll_audio(&mut audio, &mut audio_done)?;

        let input = read_input_events()?;
        let input_at = Instant::now();
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
                Ok(Some(path)) => {
                    let key = normalized_subtitle_path(&path);
                    if let Some(index) = external_subtitle_paths
                        .iter()
                        .find_map(|(loaded_path, index)| (loaded_path == &key).then_some(*index))
                    {
                        selected_subtitle = Some(index);
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
                        match load_dropped_subtitle_track(&path) {
                            Ok(track) => {
                                let index = subtitle_tracks.len();
                                subtitle_labels
                                    .push(Box::leak(track.label().to_string().into_boxed_str()));
                                subtitle_tracks.push(track);
                                external_subtitle_paths.push((key, index));
                                selected_subtitle = Some(index);
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
            PlaybackCommand::Quit => break,
            PlaybackCommand::TogglePause => {
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                toggle_pause(&mut paused, &decoder, &mut audio, &mut next_frame_at);
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
                    status_message = Some(StatusMessage {
                        text: "SUBTITLES OFF",
                        visible_until: input_at + STATUS_VISIBLE_FOR,
                    });
                } else {
                    selected_subtitle = Some(0);
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
            PlaybackCommand::SeekBy(seconds) => {
                scrub_position = None;
                let seek_target = seek_position(playback_position, seconds, source.duration);
                if is_end_seek(seek_target, source.duration) {
                    break;
                }
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                seek_playback(
                    &path,
                    source.has_audio,
                    &mut decoder,
                    &mut audio,
                    &mut audio_done,
                    seek_target,
                    paused,
                    muted,
                )?;
                playback_position = seek_target;
                video_ended = false;
                next_frame_at = Instant::now();
                redraw_current_frame = false;
            }
            PlaybackCommand::None => {}
        }

        let (current_target, current_canvas) =
            terminal_target_and_canvas(source.width, source.height);
        if current_target != target {
            if !paused && let Some(audio) = audio.as_mut() {
                audio.set_paused(true);
            }

            decoder.stop()?;
            target = current_target;
            frame.resize(target.frame_len(), 0);
            decoder = VideoDecoder::spawn_at(
                &path,
                target.width,
                target.height,
                source.fps,
                playback_position,
                paused,
            )?;

            if !paused && let Some(audio) = audio.as_mut() {
                audio.set_paused(false);
            }

            clear_screen_and_images(&mut out)?;
            previous_image_id = None;
            have_frame = false;
            redraw_current_frame = false;
            last_drawn_overlay_visible = false;
            last_drawn_status_visible = false;
            scrub_position = None;
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
            if paused && have_frame {
                let state = overlay_state(
                    playback_position,
                    scrub_position,
                    source.duration,
                    paused,
                    overlay_visible_until,
                    status_message,
                    !subtitle_tracks.is_empty(),
                    selected_subtitle,
                    subtitle_picker_open,
                    subtitle_labels.clone(),
                    media_title,
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
                redraw_current_frame = false;
            }
        }

        let mut pointer_seek_target = None;
        let hit_context = OverlayHitContext {
            width: canvas.width,
            height: canvas.height,
            scale_percent: canvas.overlay_scale_percent,
            duration: source.duration,
            subtitles_available: !subtitle_tracks.is_empty(),
            subtitle_count: subtitle_tracks.len(),
        };
        for mouse in input.mouse_events {
            let seek_target = match mouse {
                PlaybackMouse::Down { column, row } => {
                    let point = mouse_canvas_position(column, row, canvas);
                    if let Some(action) = point.and_then(|point| {
                        overlay.subtitle_picker_action(hit_context, point, subtitle_picker_open)
                    }) {
                        scrub_position = None;
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        match action {
                            SubtitlePickerAction::TogglePicker => {
                                subtitle_picker_open = !subtitle_picker_open;
                            }
                            SubtitlePickerAction::SelectTrack(index) => {
                                selected_subtitle = Some(index);
                                subtitle_picker_open = false;
                                subtitle_renderer = SubtitleRenderer::new(
                                    font_system,
                                    active_subtitle_track(&subtitle_tracks, selected_subtitle)
                                        .and_then(SubtitleTrack::language),
                                );
                            }
                            SubtitlePickerAction::SelectOff => {
                                selected_subtitle = None;
                                subtitle_picker_open = false;
                            }
                        }
                        redraw_current_frame = have_frame;
                    } else if point
                        .is_some_and(|point| overlay.playback_button_hit_test(hit_context, point))
                    {
                        scrub_position = None;
                        subtitle_picker_open = false;
                        overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                        toggle_pause(&mut paused, &decoder, &mut audio, &mut next_frame_at);
                        redraw_current_frame = have_frame;
                    } else {
                        subtitle_picker_open = false;
                        scrub_position = point
                            .and_then(|point| overlay.progress_hit_test(hit_context, point))
                            .and_then(|ratio| seek_from_progress_ratio(ratio, source.duration));
                        if scrub_position.is_some() {
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
                        canvas.overlay_scale_percent,
                        source.duration,
                        !subtitle_tracks.is_empty(),
                        x,
                    );
                    scrub_position = seek_from_progress_ratio(ratio, source.duration);
                    redraw_current_frame = have_frame;
                    None
                }
                PlaybackMouse::Up { column, row } if scrub_position.is_some() => {
                    let x = mouse_canvas_x(column, row, canvas);
                    let ratio = overlay.progress_ratio_from_x(
                        canvas.width,
                        canvas.height,
                        canvas.overlay_scale_percent,
                        source.duration,
                        !subtitle_tracks.is_empty(),
                        x,
                    );
                    let target = seek_from_progress_ratio(ratio, source.duration);
                    scrub_position = None;
                    target
                }
                PlaybackMouse::Up { .. } => {
                    scrub_position = None;
                    None
                }
                _ => None,
            };

            if let Some(seek_target) = seek_target {
                pointer_seek_target = Some(seek_target);
            }
        }

        if let Some(seek_target) = pointer_seek_target {
            if is_end_seek(seek_target, source.duration) {
                break;
            }
            seek_playback(
                &path,
                source.has_audio,
                &mut decoder,
                &mut audio,
                &mut audio_done,
                seek_target,
                paused,
                muted,
            )?;
            playback_position = seek_target;
            video_ended = false;
            next_frame_at = Instant::now();
            redraw_current_frame = false;
        }

        let overlay_is_visible = overlay_visible(
            paused,
            scrub_position.is_some(),
            overlay_visible_until,
            Instant::now(),
        );
        let status_is_visible = status_text(status_message, Instant::now()).is_some();
        if have_frame
            && ((last_drawn_overlay_visible && !overlay_is_visible)
                || (last_drawn_status_visible && !status_is_visible))
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
                !subtitle_tracks.is_empty(),
                selected_subtitle,
                subtitle_picker_open,
                subtitle_labels.clone(),
                media_title,
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
            redraw_current_frame = false;
            out.flush()?;
        }

        if paused {
            match decoder.read_latest_frame(&mut frame)? {
                FrameStatus::NewFrame { pts } => {
                    playback_position = pts;
                    let state = overlay_state(
                        playback_position,
                        scrub_position,
                        source.duration,
                        paused,
                        overlay_visible_until,
                        status_message,
                        !subtitle_tracks.is_empty(),
                        selected_subtitle,
                        subtitle_picker_open,
                        subtitle_labels.clone(),
                        media_title,
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
                    redraw_current_frame = false;
                }
                FrameStatus::NoFrame => {}
                FrameStatus::Ended => {
                    video_ended = true;
                }
            }
            out.flush()?;
            thread::sleep(Duration::from_millis(15));
            continue;
        }

        let now = Instant::now();
        if now < next_frame_at {
            out.flush()?;
            thread::sleep((next_frame_at - now).min(Duration::from_millis(5)));
            continue;
        }

        match decoder.read_latest_frame(&mut frame)? {
            FrameStatus::NewFrame { pts } => {
                playback_position = pts;
                let state = overlay_state(
                    playback_position,
                    scrub_position,
                    source.duration,
                    paused,
                    overlay_visible_until,
                    status_message,
                    !subtitle_tracks.is_empty(),
                    selected_subtitle,
                    subtitle_picker_open,
                    subtitle_labels.clone(),
                    media_title,
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
                redraw_current_frame = false;
                out.flush()?;
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
                }
                if have_frame {
                    let state = overlay_state(
                        playback_position,
                        scrub_position,
                        source.duration,
                        paused,
                        overlay_visible_until,
                        status_message,
                        !subtitle_tracks.is_empty(),
                        selected_subtitle,
                        subtitle_picker_open,
                        subtitle_labels.clone(),
                        media_title,
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
                    redraw_current_frame = false;
                }
                out.flush()?;
                thread::sleep(Duration::from_millis(10));
            }
        }

        if video_ended && audio_done {
            break;
        }
    }

    decoder.stop()?;
    if let Some(audio) = audio.as_mut() {
        audio.stop()?;
    }
    Ok(())
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
    position: Duration,
    paused: bool,
    muted: bool,
) -> Result<()> {
    decoder.seek(position);
    if has_audio {
        if let Some(audio) = audio.as_mut() {
            audio.seek(position);
            audio.set_paused(paused);
            audio.set_muted(muted);
        } else {
            *audio = Some(AudioPlayer::spawn_at(path, position, paused, muted)?);
        }
        *audio_done = false;
    }
    Ok(())
}

fn toggle_pause(
    paused: &mut bool,
    decoder: &VideoDecoder,
    audio: &mut Option<AudioPlayer>,
    next_frame_at: &mut Instant,
) {
    *paused = !*paused;
    decoder.set_paused(*paused);
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
    subtitles_available: bool,
    selected_subtitle: Option<usize>,
    subtitle_picker_open: bool,
    subtitle_labels: Vec<&'static str>,
    media_title: &'static str,
) -> OverlayState {
    let now = Instant::now();
    OverlayState {
        position: scrub_position.unwrap_or(position),
        duration,
        paused,
        visible: overlay_visible(paused, scrub_position.is_some(), visible_until, now)
            || subtitle_picker_open,
        subtitles_available,
        selected_subtitle,
        subtitle_picker_open,
        subtitle_labels,
        status_message: status_text(status_message, now),
        media_title: Some(media_title),
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

fn active_subtitle_track(
    tracks: &[SubtitleTrack],
    selected_subtitle: Option<usize>,
) -> Option<&SubtitleTrack> {
    selected_subtitle.and_then(|index| tracks.get(index))
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

fn load_subtitle_tracks(media_path: &Path, sub_file: Option<&Path>) -> Result<Vec<SubtitleTrack>> {
    let mut tracks = Vec::new();
    if let Some(path) = external_subtitle_path(media_path, sub_file) {
        tracks.push(SubtitleTrack::load(&path)?);
    }
    tracks.extend(load_embedded_subtitle_tracks(media_path)?);
    Ok(tracks)
}

fn external_subtitle_path(media_path: &Path, sub_file: Option<&Path>) -> Option<PathBuf> {
    sub_file
        .map(Path::to_path_buf)
        .or_else(|| sidecar_subtitle_path(media_path))
}

fn normalized_subtitle_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn subtitle_path_from_drop_text(text: &str) -> Result<Option<PathBuf>> {
    for candidate in media_candidates_from_text(text) {
        if !is_supported_subtitle_path(&candidate) {
            continue;
        }
        validate_subtitle_path(&candidate)?;
        return Ok(Some(candidate));
    }
    Ok(None)
}

fn load_dropped_subtitle_track(path: &Path) -> Result<SubtitleTrack> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("External");
    Ok(SubtitleTrack::load(path)?.with_label(format!("External — {file_name}")))
}

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn is_supported_subtitle_path(path: &Path) -> bool {
    ["srt", "ass", "ssa", "vtt"]
        .iter()
        .any(|extension| path_extension_is(path, extension))
}

struct Config {
    path: Option<PathBuf>,
    force: bool,
    sub_file: Option<PathBuf>,
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Config> {
    let args = args.collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }

    let mut force = false;
    let mut sub_file = None::<PathBuf>;
    let mut positionals = Vec::<OsString>::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--sub-file" {
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("--sub-file requires a path"))?;
            let path = PathBuf::from(value);
            validate_subtitle_path(&path)?;
            sub_file = Some(path);
            continue;
        }
        let arg_text = arg.to_string_lossy();
        if let Some(value) = arg_text.strip_prefix("--sub-file=") {
            let path = PathBuf::from(value);
            validate_subtitle_path(&path)?;
            sub_file = Some(path);
            continue;
        }

        if arg_text.starts_with('-') && positionals.is_empty() {
            bail!("unknown argument: {}", arg_text);
        }
        drop(arg_text);

        positionals.push(arg);
    }

    let path = join_positionals(positionals)
        .map(media_path_from_argument)
        .transpose()?;

    Ok(Config {
        path,
        force,
        sub_file,
    })
}

fn media_path_from_argument(path: PathBuf) -> Result<PathBuf> {
    let text = path.as_os_str().to_string_lossy();
    let path = media_candidates_from_text(&text)
        .into_iter()
        .next()
        .unwrap_or(path);
    validate_media_path(&path)?;
    Ok(path)
}

fn media_path_from_drop_text(text: &str) -> Result<PathBuf> {
    let candidates = media_candidates_from_text(text);
    if candidates.is_empty() {
        bail!("drop a video file or URL to play");
    }

    let mut last_error = None::<String>;
    for candidate in candidates {
        match validate_media_path(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) => last_error = Some(error.to_string()),
        }
    }

    bail!(
        "{}",
        last_error.unwrap_or_else(|| "drop a video file or URL to play".to_string())
    )
}

fn validate_media_path(path: &Path) -> Result<()> {
    let text = path.as_os_str().to_string_lossy();
    if is_remote_url_text(&text) {
        return Ok(());
    }
    if !path.exists() {
        bail!(
            "video does not exist: {}. If the path contains spaces, quote it.",
            path.display()
        );
    }
    if !path.is_file() {
        bail!("video path is not a file: {}", path.display());
    }
    Ok(())
}

fn validate_subtitle_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("subtitle file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("subtitle path is not a file: {}", path.display());
    }
    Ok(())
}

fn join_positionals(positionals: Vec<OsString>) -> Option<PathBuf> {
    let mut iter = positionals.into_iter();
    let first = iter.next()?;
    let mut path = first;
    for part in iter {
        path.push(" ");
        path.push(part);
    }
    Some(PathBuf::from(path))
}

fn print_help() {
    println!(
        "\
rigoberto - video player for Kitty-compatible terminals

Usage:
  rigoberto [--force] [--sub-file subtitle] [video-or-url]

Controls:
  Drop file/URL      play from launcher
  Space, right click  pause/play
  m                  mute/unmute
  v                  subtitles on/off
  Left, Right         seek 5 seconds
  q                  quit
"
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TargetFrame {
    width: u32,
    height: u32,
}

impl TargetFrame {
    fn frame_len(self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CanvasFrame {
    width: u32,
    height: u32,
    terminal_width: u32,
    terminal_height: u32,
    video_x: u32,
    video_y: u32,
    video_width: u32,
    video_height: u32,
    overlay_scale_percent: u32,
    area: ImageArea,
}

impl CanvasFrame {
    fn frame_len(self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

fn terminal_target_and_canvas(source_width: u32, source_height: u32) -> (TargetFrame, CanvasFrame) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let cols = cols.max(1);
    let rows = rows.max(1);
    let (pixel_width, pixel_height) = terminal_pixel_size(cols, rows);
    let target = target_for_bounds(source_width, source_height, pixel_width, pixel_height);
    let canvas = canvas_for_terminal(
        source_width,
        source_height,
        cols,
        rows,
        pixel_width,
        pixel_height,
    );
    (target, canvas)
}

fn canvas_for_terminal(
    source_width: u32,
    source_height: u32,
    cols: u16,
    rows: u16,
    pixel_width: u32,
    pixel_height: u32,
) -> CanvasFrame {
    let canvas = cap_pixels(
        pixel_width.max(1),
        pixel_height.max(1),
        MAX_CANVAS_WIDTH,
        MAX_CANVAS_HEIGHT,
    );
    let video = fit_pixels(source_width, source_height, canvas.width, canvas.height);
    let video_x = canvas.width.saturating_sub(video.width) / 2;
    let video_y = canvas.height.saturating_sub(video.height) / 2;
    let overlay_scale_percent =
        overlay_scale_percent(pixel_width, pixel_height, canvas.width, canvas.height);

    CanvasFrame {
        width: canvas.width,
        height: canvas.height,
        terminal_width: pixel_width.max(1),
        terminal_height: pixel_height.max(1),
        video_x,
        video_y,
        video_width: video.width,
        video_height: video.height,
        overlay_scale_percent,
        area: ImageArea {
            x: 0,
            y: 0,
            cols,
            rows,
        },
    }
}

fn target_for_bounds(
    source_width: u32,
    source_height: u32,
    pixel_width: u32,
    pixel_height: u32,
) -> TargetFrame {
    let max_width = pixel_width.min(MAX_DECODE_WIDTH).min(source_width).max(1);
    let max_height = pixel_height
        .min(MAX_DECODE_HEIGHT)
        .min(source_height)
        .max(1);
    let capped = fit_pixels(source_width, source_height, max_width, max_height);

    TargetFrame {
        width: capped.width.max(1),
        height: capped.height.max(1),
    }
}

#[derive(Clone, Copy)]
struct PixelSize {
    width: u32,
    height: u32,
}

fn fit_pixels(source_width: u32, source_height: u32, max_width: u32, max_height: u32) -> PixelSize {
    let source_aspect = f64::from(source_width.max(1)) / f64::from(source_height.max(1));
    let max_aspect = f64::from(max_width.max(1)) / f64::from(max_height.max(1));

    let (width, height) = if max_aspect > source_aspect {
        (
            (f64::from(max_height) * source_aspect).round() as u32,
            max_height,
        )
    } else {
        (
            max_width,
            (f64::from(max_width) / source_aspect).round() as u32,
        )
    };

    PixelSize {
        width: width.max(1),
        height: height.max(1),
    }
}

fn cap_pixels(width: u32, height: u32, max_width: u32, max_height: u32) -> PixelSize {
    fit_pixels(
        width,
        height,
        width.min(max_width).max(1),
        height.min(max_height).max(1),
    )
}

fn overlay_scale_percent(
    pixel_width: u32,
    pixel_height: u32,
    canvas_width: u32,
    canvas_height: u32,
) -> u32 {
    let width_scale = f64::from(pixel_width.max(1)) / f64::from(canvas_width.max(1));
    let height_scale = f64::from(pixel_height.max(1)) / f64::from(canvas_height.max(1));
    let canvas_scale = width_scale.max(height_scale).max(1.0);
    let boost = ((canvas_scale - 1.0) * 40.0).round() as u32;

    NORMAL_OVERLAY_SCALE_PERCENT
        .saturating_add(boost)
        .clamp(NORMAL_OVERLAY_SCALE_PERCENT, MAX_OVERLAY_SCALE_PERCENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_shell_split_path_parts() {
        let path = join_positionals(vec![
            OsString::from("/tmp/La"),
            OsString::from("fascinante"),
            OsString::from("historia.mp4"),
        ])
        .expect("path should be reconstructed");

        assert_eq!(path, PathBuf::from("/tmp/La fascinante historia.mp4"));
    }

    #[test]
    fn parse_args_accepts_launcher_without_path() {
        let config = parse_args(Vec::<OsString>::new().into_iter()).expect("args should parse");

        assert_eq!(config.path, None);
        assert!(!config.force);
        assert_eq!(config.sub_file, None);
    }

    #[test]
    fn launcher_drop_uses_same_media_and_sidecar_path_as_argument() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-app-drop-subtitle-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("Fabricated City.mkv");
        let sidecar = temp_dir.join("Fabricated City.srt");
        std::fs::write(&media, "video").expect("video should be written");
        std::fs::write(&sidecar, "subtitle").expect("subtitle should be written");

        let from_arg = media_path_from_argument(media.clone()).expect("arg media should parse");
        let from_drop = media_path_from_drop_text(&media.display().to_string())
            .expect("drop media should parse");

        assert_eq!(from_drop, from_arg);
        assert_eq!(sidecar_subtitle_path(&from_arg), Some(sidecar.clone()));
        assert_eq!(sidecar_subtitle_path(&from_drop), Some(sidecar));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn playback_drop_accepts_subtitle_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-app-playback-subtitle-drop-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub_file = temp_dir.join("Movie Signs.eng.ass");
        std::fs::write(&sub_file, "subtitle").expect("subtitle should be written");

        let from_drop = subtitle_path_from_drop_text(&format!("file://{}", sub_file.display()))
            .expect("drop subtitle should parse");

        assert_eq!(from_drop, Some(sub_file));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn playback_drop_normalizes_duplicate_subtitle_paths() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-app-playback-subtitle-dup-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub_file = temp_dir.join("movie.srt");
        std::fs::write(&sub_file, "subtitle").expect("subtitle should be written");

        let plain = subtitle_path_from_drop_text(&sub_file.display().to_string())
            .expect("plain subtitle should parse")
            .expect("plain subtitle should exist");
        let file_url = subtitle_path_from_drop_text(&format!("file://{}", sub_file.display()))
            .expect("file url subtitle should parse")
            .expect("file url subtitle should exist");

        assert_eq!(
            normalized_subtitle_path(&plain),
            normalized_subtitle_path(&file_url)
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn playback_drop_ignores_non_subtitle_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-app-playback-video-drop-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let media = temp_dir.join("Movie.mkv");
        std::fs::write(&media, "video").expect("video should be written");

        let from_drop = subtitle_path_from_drop_text(&media.display().to_string())
            .expect("video drop should not error");

        assert_eq!(from_drop, None);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_args_accepts_remote_url() {
        let config = parse_args(vec![OsString::from("https://example.com/video.mp4")].into_iter())
            .expect("url should parse");

        assert_eq!(
            config.path,
            Some(PathBuf::from("https://example.com/video.mp4"))
        );
        assert_eq!(config.sub_file, None);
    }

    #[test]
    fn parse_args_accepts_sub_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rigoberto-app-subtitle-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub_file = temp_dir.join("movie.srt");
        std::fs::write(&sub_file, "").expect("subtitle should be written");

        let config = parse_args(
            vec![
                OsString::from("--sub-file"),
                sub_file.clone().into_os_string(),
                OsString::from("https://example.com/video.mp4"),
            ]
            .into_iter(),
        )
        .expect("args should parse");

        assert_eq!(
            config.path,
            Some(PathBuf::from("https://example.com/video.mp4"))
        );
        assert_eq!(config.sub_file, Some(sub_file));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn target_caps_large_sources_at_1080p() {
        let target = target_for_bounds(3840, 2160, 3840, 2160);

        assert_eq!(target.width, 1920);
        assert_eq!(target.height, 1080);
    }

    #[test]
    fn target_does_not_upscale_small_sources() {
        let target = target_for_bounds(1280, 720, 3840, 2160);

        assert_eq!(target.width, 1280);
        assert_eq!(target.height, 720);
    }

    #[test]
    fn target_preserves_aspect_inside_1080p_cap() {
        let target = target_for_bounds(2560, 1080, 3840, 2160);

        assert_eq!(target.width, 1920);
        assert_eq!(target.height, 810);
    }

    #[test]
    fn seek_backward_saturates_at_start() {
        assert_eq!(
            seek_position(Duration::from_secs(3), -5, None),
            Duration::ZERO
        );
    }

    #[test]
    fn seek_forward_clamps_to_duration() {
        assert_eq!(
            seek_position(Duration::from_secs(18), 5, Some(Duration::from_secs(20))),
            Duration::from_secs(20)
        );
    }

    #[test]
    fn exact_duration_seek_is_end_seek() {
        assert!(is_end_seek(
            Duration::from_secs(20),
            Some(Duration::from_secs(20))
        ));
    }

    #[test]
    fn before_duration_seek_is_not_end_seek() {
        assert!(!is_end_seek(
            Duration::from_secs(19),
            Some(Duration::from_secs(20))
        ));
    }

    #[test]
    fn overlay_is_visible_while_paused() {
        let now = Instant::now();

        assert!(overlay_visible(true, false, None, now));
    }

    #[test]
    fn overlay_visibility_expires_when_playing() {
        let now = Instant::now();

        assert!(overlay_visible(
            false,
            false,
            Some(now + Duration::from_secs(1)),
            now
        ));
        assert!(!overlay_visible(
            false,
            false,
            Some(now - Duration::from_secs(1)),
            now
        ));
    }

    #[test]
    fn overlay_is_visible_while_scrubbing() {
        let now = Instant::now();

        assert!(overlay_visible(false, true, None, now));
    }

    #[test]
    fn overlay_state_uses_scrub_position() {
        let state = overlay_state(
            Duration::from_secs(10),
            Some(Duration::from_secs(30)),
            Some(Duration::from_secs(60)),
            false,
            None,
            None,
            false,
            None,
            false,
            Vec::new(),
            "movie.mp4",
        );

        assert_eq!(state.position, Duration::from_secs(30));
        assert!(state.visible);
        assert_eq!(state.status_message, None);
        assert_eq!(state.media_title, Some("movie.mp4"));
    }

    #[test]
    fn canvas_uses_terminal_letterbox_space() {
        let canvas = canvas_for_terminal(1280, 536, 80, 24, 1920, 1080);

        assert_eq!(
            canvas,
            CanvasFrame {
                width: 1920,
                height: 1080,
                terminal_width: 1920,
                terminal_height: 1080,
                video_x: 0,
                video_y: 138,
                video_width: 1920,
                video_height: 804,
                overlay_scale_percent: 100,
                area: ImageArea {
                    x: 0,
                    y: 0,
                    cols: 80,
                    rows: 24,
                },
            }
        );
    }

    #[test]
    fn canvas_caps_high_density_terminals() {
        let canvas = canvas_for_terminal(1280, 536, 120, 40, 2880, 1800);

        assert_eq!(
            canvas,
            CanvasFrame {
                width: 1920,
                height: 1200,
                terminal_width: 2880,
                terminal_height: 1800,
                video_x: 0,
                video_y: 198,
                video_width: 1920,
                video_height: 804,
                overlay_scale_percent: 120,
                area: ImageArea {
                    x: 0,
                    y: 0,
                    cols: 120,
                    rows: 40,
                },
            }
        );
    }

    #[test]
    fn mouse_position_maps_terminal_cell_to_canvas_pixel() {
        let canvas = CanvasFrame {
            width: 1920,
            height: 1080,
            terminal_width: 1920,
            terminal_height: 1080,
            video_x: 0,
            video_y: 138,
            video_width: 1920,
            video_height: 804,
            overlay_scale_percent: 100,
            area: ImageArea {
                x: 0,
                y: 0,
                cols: 80,
                rows: 24,
            },
        };

        let point = mouse_canvas_position(40, 20, canvas).expect("point should be inside");

        assert_eq!(point.x, 972);
        assert_eq!(point.cell.left, point.x);
        assert_eq!(point.cell.right, point.x);
        assert_eq!(point.cell.top, 922);
        assert_eq!(point.cell.bottom, 922);
    }

    #[test]
    fn mouse_position_maps_pixel_mouse_to_canvas_pixel() {
        let canvas = CanvasFrame {
            width: 1920,
            height: 1200,
            terminal_width: 2880,
            terminal_height: 1800,
            video_x: 0,
            video_y: 198,
            video_width: 1920,
            video_height: 804,
            overlay_scale_percent: 120,
            area: ImageArea {
                x: 0,
                y: 0,
                cols: 120,
                rows: 40,
            },
        };

        let point = mouse_canvas_position(1440, 1500, canvas).expect("point should be inside");

        assert_eq!(point.x, 960);
        assert_eq!(point.cell.left, 960);
        assert_eq!(point.cell.top, 1000);
        assert_eq!(point.cell.right, 960);
        assert_eq!(point.cell.bottom, 1000);
    }

    #[test]
    fn copy_video_places_frame_inside_canvas() {
        let target = TargetFrame {
            width: 2,
            height: 2,
        };
        let canvas = CanvasFrame {
            width: 4,
            height: 4,
            terminal_width: 4,
            terminal_height: 4,
            video_x: 1,
            video_y: 1,
            video_width: 2,
            video_height: 2,
            overlay_scale_percent: 100,
            area: ImageArea {
                x: 0,
                y: 0,
                cols: 4,
                rows: 4,
            },
        };
        let frame = vec![
            1, 2, 3, 4, 5, 6, //
            7, 8, 9, 10, 11, 12,
        ];
        let mut canvas_frame = vec![0_u8; canvas.frame_len()];

        copy_video_into_canvas(&frame, target, &mut canvas_frame, canvas);

        let row_bytes = canvas.width as usize * 3;
        assert_eq!(
            &canvas_frame[row_bytes + 3..row_bytes + 9],
            &[1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            &canvas_frame[row_bytes * 2 + 3..row_bytes * 2 + 9],
            &[7, 8, 9, 10, 11, 12]
        );
        assert_eq!(&canvas_frame[..3], &[0, 0, 0]);
    }

    #[test]
    fn copy_video_scales_frame_inside_canvas() {
        let target = TargetFrame {
            width: 2,
            height: 1,
        };
        let canvas = CanvasFrame {
            width: 4,
            height: 2,
            terminal_width: 4,
            terminal_height: 2,
            video_x: 0,
            video_y: 0,
            video_width: 4,
            video_height: 2,
            overlay_scale_percent: 100,
            area: ImageArea {
                x: 0,
                y: 0,
                cols: 4,
                rows: 2,
            },
        };
        let frame = vec![1, 2, 3, 7, 8, 9];
        let mut canvas_frame = vec![0_u8; canvas.frame_len()];

        copy_video_into_canvas(&frame, target, &mut canvas_frame, canvas);

        assert_eq!(&canvas_frame[..12], &[1, 2, 3, 1, 2, 3, 7, 8, 9, 7, 8, 9]);
        assert_eq!(&canvas_frame[12..24], &[1, 2, 3, 1, 2, 3, 7, 8, 9, 7, 8, 9]);
    }

    #[test]
    fn progress_ratio_seek_uses_duration() {
        assert_eq!(
            seek_from_progress_ratio(0.25, Some(Duration::from_secs(80))),
            Some(Duration::from_secs(20))
        );
    }
}
