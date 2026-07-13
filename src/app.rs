use std::{
    env,
    ffi::OsString,
    io::{self, BufWriter, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use crate::{
    media::{AudioPlayer, FrameStatus, VideoDecoder, probe_video},
    terminal::{
        ImageArea, KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, TerminalGuard,
        clear_screen_and_images, enable_tmux_passthrough, inside_tmux, looks_like_kitty,
        terminal_image_area, write_kitty_rgb_frame,
    },
};

const MAX_DECODE_WIDTH: u32 = 1920;
const MAX_DECODE_HEIGHT: u32 = 1080;

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
    let target = terminal_target(source.width, source.height);

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
    clear_screen_and_images(&mut out)?;

    let mut frame = vec![0_u8; target.frame_len()];
    let mut last_layout = None::<ImageArea>;
    let mut previous_image_id = None;
    let mut frame_serial = 0_u32;
    let mut video_ended = false;
    let frame_interval = frame_interval(source.fps);
    let mut next_frame_at = playback_started_at;

    loop {
        if should_quit()? {
            break;
        }
        poll_audio(&mut audio, &mut audio_done)?;

        let layout = terminal_image_area(target.width, target.height);
        if last_layout != Some(layout) {
            clear_screen_and_images(&mut out)?;
            last_layout = Some(layout);
            previous_image_id = None;
        }

        let now = Instant::now();
        if now < next_frame_at {
            out.flush()?;
            thread::sleep((next_frame_at - now).min(Duration::from_millis(5)));
            continue;
        }

        match decoder.read_latest_frame(&mut frame)? {
            FrameStatus::NewFrame => {
                let image_id = KITTY_IMAGE_IDS[(frame_serial as usize) % KITTY_IMAGE_IDS.len()];
                write_kitty_rgb_frame(
                    &mut out,
                    KittyFramePlacement {
                        image_id,
                        placement_id: KITTY_PLACEMENT_ID,
                        z_index: 0,
                        previous_image_id,
                        width: target.width,
                        height: target.height,
                        area: layout,
                    },
                    &frame,
                    &mut sequence,
                )?;
                previous_image_id = Some(image_id);
                frame_serial = frame_serial.wrapping_add(1);
                out.flush()?;
                advance_frame_clock(&mut next_frame_at, frame_interval);
            }
            FrameStatus::NoFrame => {
                out.flush()?;
                thread::sleep(Duration::from_millis(2));
            }
            FrameStatus::Ended => {
                video_ended = true;
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
  q, Esc, Ctrl-C    quit playback
"
    );
}

#[derive(Clone, Copy)]
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

fn should_quit() -> Result<bool> {
    while event::poll(Duration::from_millis(0)).context("failed to poll terminal input")? {
        if let Event::Key(key) = event::read().context("failed to read terminal input")? {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                || (matches!(key.code, KeyCode::Char('c'))
                    && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(true);
            }
        }
    }

    Ok(false)
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
}
