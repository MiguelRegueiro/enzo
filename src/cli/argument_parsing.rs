use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use crate::{
    config::{Config, MAX_VOLUME_MAX, MIN_VOLUME_MAX},
    media_source::{media_path_from_argument, validate_subtitle_path},
};

pub(crate) const HELP: &str = "\
enzo - terminal video player

Usage:
  enzo [OPTIONS] [VIDEO-OR-URL]

Options:
  -h, --help                     Print help
  -V, --version                  Print version
      --force                    Bypass Kitty terminal detection
      --force-media-title TITLE  Override the displayed title
      --sub-file PATH            Load an external subtitle file
      --config PATH              Load configuration from a custom path
      --volume-max PERCENT       Set maximum volume (100-1000; default: 100)
      --resume                   Enable resume data
      --no-resume                Disable resume data
      --autoplay-next            Play next video when playback ends
      --no-autoplay-next         Do not play next video when playback ends
      --clear-resume             Remove saved playback state and exit
";

pub(crate) const VERSION: &str = concat!("enzo ", env!("CARGO_PKG_VERSION"));

pub(crate) enum Action {
    Run(Options),
    Help,
    Version,
}

pub(crate) struct Options {
    pub(crate) path: Option<PathBuf>,
    pub(crate) force: bool,
    pub(crate) force_media_title: Option<String>,
    pub(crate) sub_file: Option<PathBuf>,
    pub(crate) volume_max: u16,
    pub(crate) resume_enabled: bool,
    pub(crate) autoplay_next: bool,
    pub(crate) accent_color: [u8; 3],
    pub(crate) clear_resume: bool,
}

pub(crate) fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Action> {
    parse_args_with_config_loader(args, Config::load)
}

fn parse_args_with_config_loader(
    args: impl Iterator<Item = OsString>,
    load_config: impl FnOnce(Option<&Path>) -> Result<Config>,
) -> Result<Action> {
    let args = args.collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Action::Help);
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Ok(Action::Version);
    }
    let mut force = false;
    let mut force_media_title = None::<String>;
    let mut sub_file = None::<PathBuf>;
    let mut config_file = None::<PathBuf>;
    let mut volume_max = None::<u16>;
    let mut resume_enabled = None::<bool>;
    let mut autoplay_next = None::<bool>;
    let mut clear_resume = false;
    let mut positionals = Vec::<OsString>::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--force-media-title" {
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("--force-media-title requires a title"))?;
            force_media_title = Some(value.to_string_lossy().into_owned());
            continue;
        }
        if arg == "--resume" {
            resume_enabled = Some(true);
            continue;
        }
        if arg == "--no-resume" {
            resume_enabled = Some(false);
            continue;
        }
        if arg == "--autoplay-next" {
            autoplay_next = Some(true);
            continue;
        }
        if arg == "--no-autoplay-next" {
            autoplay_next = Some(false);
            continue;
        }
        if arg == "--clear-resume" {
            clear_resume = true;
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
        if arg == "--config" {
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("--config requires a path"))?;
            config_file = Some(PathBuf::from(value));
            continue;
        }
        if arg == "--volume-max" {
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("--volume-max requires a value"))?;
            volume_max = Some(parse_volume_max(&value)?);
            continue;
        }
        let arg_text = arg.to_string_lossy();
        if let Some(value) = arg_text.strip_prefix("--force-media-title=") {
            force_media_title = Some(value.to_owned());
            continue;
        }
        if let Some(value) = arg_text.strip_prefix("--sub-file=") {
            let path = PathBuf::from(value);
            validate_subtitle_path(&path)?;
            sub_file = Some(path);
            continue;
        }
        if let Some(value) = arg_text.strip_prefix("--config=") {
            config_file = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = arg_text.strip_prefix("--volume-max=") {
            volume_max = Some(parse_volume_max(value)?);
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
    if clear_resume && (path.is_some() || sub_file.is_some()) {
        bail!("--clear-resume cannot be combined with media or subtitle paths");
    }
    let config = load_config(config_file.as_deref())?;

    Ok(Action::Run(Options {
        path,
        force,
        force_media_title,
        sub_file,
        volume_max: volume_max.unwrap_or(config.volume_max),
        resume_enabled: resume_enabled.unwrap_or(config.resume),
        autoplay_next: autoplay_next.unwrap_or(config.autoplay_next),
        accent_color: config.accent_color,
        clear_resume,
    }))
}

fn parse_volume_max(value: impl AsRef<std::ffi::OsStr>) -> Result<u16> {
    let value = value.as_ref().to_string_lossy();
    let percent = value
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid --volume-max value: {value}"))?;
    if !(MIN_VOLUME_MAX..=MAX_VOLUME_MAX).contains(&percent) {
        bail!("--volume-max must be between {MIN_VOLUME_MAX} and {MAX_VOLUME_MAX}");
    }
    Ok(percent)
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

#[cfg(test)]
#[path = "tests/argument_parsing.rs"]
mod tests;
