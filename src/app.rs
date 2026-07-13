use std::{
    env,
    ffi::OsString,
    io::{self, BufWriter, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    input::{PlaybackCommand, read_input_events},
    media::{AudioPlayer, FrameStatus, VideoDecoder, probe_video},
    overlay::{OverlayState, PlaybackOverlay},
    terminal::{
        ImageArea, KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, TerminalGuard,
        clear_screen_and_images, enable_tmux_passthrough, inside_tmux, looks_like_kitty,
        terminal_image_area, write_kitty_rgb_frame,
    },
};

const MAX_DECODE_WIDTH: u32 = 1920;
const MAX_DECODE_HEIGHT: u32 = 1080;
const OVERLAY_VISIBLE_FOR: Duration = Duration::from_secs(2);

pub(crate) fn run() -> Result<()> {
    let config = parse_args(env::args_os().skip(1))?;
    if !config.force && !looks_like_kitty() {
        bail!(
            "Rigoberto targets Kitty graphics; run from kitty or pass --force if your terminal is compatible"
        );
    }

    if inside_tmux() {
        enable_tmux_passthrough();
    }

    let source = probe_video(&config.path).with_context(|| {
        format!(
            "failed to inspect video metadata for {}",
            config.path.display()
        )
    })?;
    let mut target = terminal_target(source.width, source.height);

    let mut decoder = VideoDecoder::spawn(&config.path, target.width, target.height, source.fps)?;
    let mut audio = if source.has_audio {
        Some(AudioPlayer::spawn(&config.path)?)
    } else {
        None
    };
    let mut audio_done = !source.has_audio;
    let playback_started_at = Instant::now();

    let _terminal = TerminalGuard::enter()?;
    let stdout = io::stdout();
    let mut out =
        BufWriter::with_capacity(target.frame_len() + target.frame_len() / 2, stdout.lock());
    let mut sequence = Vec::with_capacity(target.frame_len() + target.frame_len() / 2 + 4096);
    let mut overlay = PlaybackOverlay::new();
    clear_screen_and_images(&mut out)?;

    let mut frame = vec![0_u8; target.frame_len()];
    let mut composited_frame = vec![0_u8; target.frame_len()];
    let mut last_layout = None::<ImageArea>;
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

    loop {
        poll_audio(&mut audio, &mut audio_done)?;

        let input = read_input_events()?;
        let input_at = Instant::now();
        let was_overlay_visible = overlay_visible(paused, overlay_visible_until, input_at);
        if input.mouse_activity {
            overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
            if have_frame && !was_overlay_visible {
                redraw_current_frame = true;
            }
        }

        match input.command {
            PlaybackCommand::Quit => break,
            PlaybackCommand::TogglePause => {
                paused = !paused;
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                decoder.set_paused(paused);
                if let Some(audio) = audio.as_mut() {
                    audio.set_paused(paused);
                }
                if !paused {
                    next_frame_at = Instant::now();
                }
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::SeekBy(seconds) => {
                let seek_target = seek_position(playback_position, seconds, source.duration);
                if is_end_seek(seek_target, source.duration) {
                    break;
                }
                overlay_visible_until = Some(input_at + OVERLAY_VISIBLE_FOR);
                decoder.seek(seek_target);
                if source.has_audio {
                    if let Some(audio) = audio.as_mut() {
                        audio.seek(seek_target);
                        audio.set_paused(paused);
                    } else {
                        audio = Some(AudioPlayer::spawn_at(&config.path, seek_target, paused)?);
                    }
                    audio_done = false;
                }
                playback_position = seek_target;
                video_ended = false;
                next_frame_at = Instant::now();
                redraw_current_frame = have_frame;
            }
            PlaybackCommand::None => {}
        }

        let current_target = terminal_target(source.width, source.height);
        if current_target != target {
            if !paused && let Some(audio) = audio.as_mut() {
                audio.set_paused(true);
            }

            decoder.stop()?;
            target = current_target;
            frame.resize(target.frame_len(), 0);
            composited_frame.resize(target.frame_len(), 0);
            decoder = VideoDecoder::spawn_at(
                &config.path,
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
            last_layout = None;
            previous_image_id = None;
            have_frame = false;
            redraw_current_frame = false;
            last_drawn_overlay_visible = false;
            video_ended = false;
            next_frame_at = Instant::now();
        }

        let layout = terminal_image_area(target.width, target.height);
        if last_layout != Some(layout) {
            clear_screen_and_images(&mut out)?;
            last_layout = Some(layout);
            previous_image_id = None;
            last_drawn_overlay_visible = false;
            if paused && have_frame {
                let state = overlay_state(
                    playback_position,
                    source.duration,
                    paused,
                    overlay_visible_until,
                );
                draw_frame(
                    &mut out,
                    target,
                    layout,
                    &mut previous_image_id,
                    &mut frame_serial,
                    &frame,
                    &mut composited_frame,
                    &mut overlay,
                    state,
                    &mut sequence,
                )?;
                last_drawn_overlay_visible = state.visible;
                redraw_current_frame = false;
            }
        }

        let overlay_is_visible = overlay_visible(paused, overlay_visible_until, Instant::now());
        if have_frame && last_drawn_overlay_visible && !overlay_is_visible {
            redraw_current_frame = true;
        }

        if redraw_current_frame && have_frame {
            let state = overlay_state(
                playback_position,
                source.duration,
                paused,
                overlay_visible_until,
            );
            draw_frame(
                &mut out,
                target,
                layout,
                &mut previous_image_id,
                &mut frame_serial,
                &frame,
                &mut composited_frame,
                &mut overlay,
                state,
                &mut sequence,
            )?;
            last_drawn_overlay_visible = state.visible;
            redraw_current_frame = false;
            out.flush()?;
        }

        if paused {
            match decoder.read_latest_frame(&mut frame)? {
                FrameStatus::NewFrame { pts } => {
                    playback_position = pts;
                    let state = overlay_state(
                        playback_position,
                        source.duration,
                        paused,
                        overlay_visible_until,
                    );
                    draw_frame(
                        &mut out,
                        target,
                        layout,
                        &mut previous_image_id,
                        &mut frame_serial,
                        &frame,
                        &mut composited_frame,
                        &mut overlay,
                        state,
                        &mut sequence,
                    )?;
                    have_frame = true;
                    last_drawn_overlay_visible = state.visible;
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
                    source.duration,
                    paused,
                    overlay_visible_until,
                );
                draw_frame(
                    &mut out,
                    target,
                    layout,
                    &mut previous_image_id,
                    &mut frame_serial,
                    &frame,
                    &mut composited_frame,
                    &mut overlay,
                    state,
                    &mut sequence,
                )?;
                have_frame = true;
                last_drawn_overlay_visible = state.visible;
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
                        source.duration,
                        paused,
                        overlay_visible_until,
                    );
                    draw_frame(
                        &mut out,
                        target,
                        layout,
                        &mut previous_image_id,
                        &mut frame_serial,
                        &frame,
                        &mut composited_frame,
                        &mut overlay,
                        state,
                        &mut sequence,
                    )?;
                    last_drawn_overlay_visible = state.visible;
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

fn draw_frame(
    out: &mut impl Write,
    target: TargetFrame,
    layout: ImageArea,
    previous_image_id: &mut Option<u32>,
    frame_serial: &mut u32,
    frame: &[u8],
    composited_frame: &mut [u8],
    overlay: &mut PlaybackOverlay,
    overlay_state: OverlayState,
    sequence: &mut Vec<u8>,
) -> io::Result<()> {
    if composited_frame.len() != frame.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "overlay scratch frame length does not match decoded frame length",
        ));
    }

    composited_frame.copy_from_slice(frame);
    overlay.render(composited_frame, target.width, target.height, overlay_state);

    let image_id = KITTY_IMAGE_IDS[(*frame_serial as usize) % KITTY_IMAGE_IDS.len()];
    write_kitty_rgb_frame(
        out,
        KittyFramePlacement {
            image_id,
            placement_id: KITTY_PLACEMENT_ID,
            z_index: 0,
            previous_image_id: *previous_image_id,
            width: target.width,
            height: target.height,
            area: layout,
        },
        composited_frame,
        sequence,
    )?;
    *previous_image_id = Some(image_id);
    *frame_serial = frame_serial.wrapping_add(1);
    Ok(())
}

fn overlay_state(
    position: Duration,
    duration: Option<Duration>,
    paused: bool,
    visible_until: Option<Instant>,
) -> OverlayState {
    OverlayState {
        position,
        duration,
        paused,
        visible: overlay_visible(paused, visible_until, Instant::now()),
    }
}

fn overlay_visible(paused: bool, visible_until: Option<Instant>, now: Instant) -> bool {
    paused || visible_until.is_some_and(|until| now < until)
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

struct Config {
    path: PathBuf,
    force: bool,
}

fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Config> {
    let args = args.collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }

    let mut force = false;
    let mut positionals = Vec::<OsString>::new();
    for arg in args {
        if arg == "--force" {
            force = true;
            continue;
        }

        if arg.to_string_lossy().starts_with('-') && positionals.is_empty() {
            bail!("unknown argument: {}", arg.to_string_lossy());
        }

        positionals.push(arg);
    }

    let path = join_positionals(positionals).ok_or_else(|| anyhow!("expected a video path"))?;
    if !path.exists() {
        bail!(
            "video does not exist: {}. If the path contains spaces, quote it.",
            path.display()
        );
    }
    if !path.is_file() {
        bail!("video path is not a file: {}", path.display());
    }

    Ok(Config { path, force })
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
  rigoberto [--force] <video>

Controls:
  Space, right click  pause/play
  Left, Right         seek 5 seconds
  q, Esc, Ctrl-C    quit playback
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

fn terminal_target(source_width: u32, source_height: u32) -> TargetFrame {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let (pixel_width, pixel_height) = crate::terminal::terminal_pixel_size(cols, rows);
    target_for_bounds(source_width, source_height, pixel_width, pixel_height)
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

        assert!(overlay_visible(true, None, now));
    }

    #[test]
    fn overlay_visibility_expires_when_playing() {
        let now = Instant::now();

        assert!(overlay_visible(
            false,
            Some(now + Duration::from_secs(1)),
            now
        ));
        assert!(!overlay_visible(
            false,
            Some(now - Duration::from_secs(1)),
            now
        ));
    }
}
