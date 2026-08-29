use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use crate::{
    config::{Config, MAX_VOLUME_MAX, MIN_VOLUME_MAX},
    media_input::{media_path_from_argument, validate_subtitle_path},
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
mod tests {
    use super::*;

    fn run_options(args: Vec<OsString>) -> Options {
        run_options_with_config(args, Config::default())
    }

    fn run_options_with_config(args: Vec<OsString>, config: Config) -> Options {
        match parse_args_with_config_loader(args.into_iter(), |_| Ok(config))
            .expect("args should parse")
        {
            Action::Run(options) => options,
            Action::Help | Action::Version => panic!("expected run options"),
        }
    }

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
        let config = run_options(Vec::new());

        assert_eq!(config.path, None);
        assert!(!config.force);
        assert_eq!(config.force_media_title, None);
        assert_eq!(config.sub_file, None);
        assert_eq!(config.volume_max, 100);
        assert!(config.resume_enabled);
        assert!(config.autoplay_next);
        assert!(!config.clear_resume);
    }

    #[test]
    fn parse_args_supports_volume_max_forms() {
        let separate = run_options(vec![OsString::from("--volume-max"), OsString::from("180")]);
        assert_eq!(separate.volume_max, 180);

        let joined = run_options(vec![OsString::from("--volume-max=250")]);
        assert_eq!(joined.volume_max, 250);
    }

    #[test]
    fn config_values_supply_playback_defaults() {
        let options = run_options_with_config(
            Vec::new(),
            Config {
                volume_max: 220,
                resume: false,
                autoplay_next: false,
            },
        );

        assert_eq!(options.volume_max, 220);
        assert!(!options.resume_enabled);
        assert!(!options.autoplay_next);
    }

    #[test]
    fn command_line_values_override_config() {
        let options = run_options_with_config(
            vec![
                OsString::from("--volume-max=180"),
                OsString::from("--resume"),
                OsString::from("--autoplay-next"),
            ],
            Config {
                volume_max: 220,
                resume: false,
                autoplay_next: false,
            },
        );

        assert_eq!(options.volume_max, 180);
        assert!(options.resume_enabled);
        assert!(options.autoplay_next);
    }

    #[test]
    fn custom_config_path_is_forwarded_to_loader() {
        let path = PathBuf::from("/tmp/custom-enzo.toml");
        let action = parse_args_with_config_loader(
            vec![OsString::from("--config"), path.clone().into_os_string()].into_iter(),
            |actual| {
                assert_eq!(actual, Some(path.as_path()));
                Ok(Config::default())
            },
        )
        .expect("custom config path should parse");

        assert!(matches!(action, Action::Run(_)));
    }

    #[test]
    fn parse_args_rejects_invalid_volume_max() {
        for value in ["99", "1001", "loud"] {
            let error =
                parse_args(vec![OsString::from(format!("--volume-max={value}"))].into_iter())
                    .err()
                    .expect("invalid maximum volume should fail");
            assert!(error.to_string().contains("--volume-max"));
        }
    }

    #[test]
    fn parse_args_supports_resume_controls() {
        let no_resume = run_options(vec![OsString::from("--no-resume")]);
        assert!(!no_resume.resume_enabled);
        assert!(!no_resume.clear_resume);

        let resume = run_options_with_config(
            vec![OsString::from("--resume")],
            Config {
                resume: false,
                ..Config::default()
            },
        );
        assert!(resume.resume_enabled);

        let clear = run_options(vec![OsString::from("--clear-resume")]);
        assert!(clear.clear_resume);
        assert!(clear.path.is_none());
    }

    #[test]
    fn parse_args_supports_autoplay_controls() {
        let config = run_options(vec![OsString::from("--no-autoplay-next")]);

        assert!(!config.autoplay_next);
        assert!(config.resume_enabled);

        let config = run_options_with_config(
            vec![OsString::from("--autoplay-next")],
            Config {
                autoplay_next: false,
                ..Config::default()
            },
        );
        assert!(config.autoplay_next);
    }

    #[test]
    fn parse_args_recognizes_help_and_version() {
        assert!(matches!(
            parse_args(vec![OsString::from("--help")].into_iter()),
            Ok(Action::Help)
        ));
        assert!(matches!(
            parse_args(vec![OsString::from("-V")].into_iter()),
            Ok(Action::Version)
        ));
    }

    #[test]
    fn parse_args_accepts_remote_url() {
        let config = run_options(vec![OsString::from("https://example.com/video.mp4")]);

        assert_eq!(
            config.path,
            Some(PathBuf::from("https://example.com/video.mp4"))
        );
        assert_eq!(config.sub_file, None);
    }

    #[test]
    fn parse_args_accepts_media_title_forms() {
        let separate = run_options(vec![
            OsString::from("--force-media-title"),
            OsString::from("Frieren Episode 1"),
            OsString::from("https://example.com/index.m3u8"),
        ]);
        assert_eq!(
            separate.force_media_title.as_deref(),
            Some("Frieren Episode 1")
        );

        let joined = run_options(vec![
            OsString::from("--force-media-title=葬送のフリーレン Episode 1"),
            OsString::from("https://example.com/index.m3u8"),
        ]);
        assert_eq!(
            joined.force_media_title.as_deref(),
            Some("葬送のフリーレン Episode 1")
        );
    }

    #[test]
    fn parse_args_rejects_media_title_without_a_value() {
        let error = parse_args(vec![OsString::from("--force-media-title")].into_iter())
            .err()
            .expect("missing title should fail");

        assert!(error.to_string().contains("requires a title"));
    }

    #[test]
    fn parse_args_accepts_sub_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-app-subtitle-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir(&temp_dir).expect("temp dir should be created");
        let sub_file = temp_dir.join("movie.srt");
        std::fs::write(&sub_file, "").expect("subtitle should be written");

        let config = run_options(vec![
            OsString::from("--sub-file"),
            sub_file.clone().into_os_string(),
            OsString::from("https://example.com/video.mp4"),
        ]);

        assert_eq!(
            config.path,
            Some(PathBuf::from("https://example.com/video.mp4"))
        );
        assert_eq!(config.sub_file, Some(sub_file));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
