use std::{ffi::OsString, path::PathBuf};

use anyhow::{Result, bail};

use crate::media_input::{media_path_from_argument, validate_subtitle_path};

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
      --no-resume                Disable reading and writing resume data
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
    pub(crate) resume_enabled: bool,
    pub(crate) autoplay_next: bool,
    pub(crate) clear_resume: bool,
}

pub(crate) fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Action> {
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
    let mut resume_enabled = true;
    let mut autoplay_next = true;
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
        if arg == "--no-resume" {
            resume_enabled = false;
            continue;
        }
        if arg == "--no-autoplay-next" {
            autoplay_next = false;
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

    Ok(Action::Run(Options {
        path,
        force,
        force_media_title,
        sub_file,
        resume_enabled,
        autoplay_next,
        clear_resume,
    }))
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
        match parse_args(args.into_iter()).expect("args should parse") {
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
        assert!(config.resume_enabled);
        assert!(config.autoplay_next);
        assert!(!config.clear_resume);
    }

    #[test]
    fn parse_args_supports_resume_controls() {
        let no_resume = run_options(vec![OsString::from("--no-resume")]);
        assert!(!no_resume.resume_enabled);
        assert!(!no_resume.clear_resume);

        let clear = run_options(vec![OsString::from("--clear-resume")]);
        assert!(clear.clear_resume);
        assert!(clear.path.is_none());
    }

    #[test]
    fn parse_args_supports_autoplay_controls() {
        let config = run_options(vec![OsString::from("--no-autoplay-next")]);

        assert!(!config.autoplay_next);
        assert!(config.resume_enabled);
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
