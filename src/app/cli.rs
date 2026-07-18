use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use super::path_input::{is_remote_url_text, media_candidates_from_text};

pub(super) struct Config {
    pub(super) path: Option<PathBuf>,
    pub(super) force: bool,
    pub(super) sub_file: Option<PathBuf>,
    pub(super) resume_enabled: bool,
    pub(super) clear_resume: bool,
}

pub(super) fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Config> {
    let args = args.collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }

    let mut force = false;
    let mut sub_file = None::<PathBuf>;
    let mut resume_enabled = true;
    let mut clear_resume = false;
    let mut positionals = Vec::<OsString>::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--no-resume" {
            resume_enabled = false;
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

    Ok(Config {
        path,
        force,
        sub_file,
        resume_enabled,
        clear_resume,
    })
}

pub(super) fn media_path_from_drop_text(text: &str) -> Result<PathBuf> {
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

pub(super) fn validate_subtitle_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("subtitle file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("subtitle path is not a file: {}", path.display());
    }
    Ok(())
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
enzo - video player for Kitty-compatible terminals

Usage:
  enzo [--force] [--no-resume] [--sub-file subtitle] [video-or-url]
  enzo --clear-resume

Controls:
  Drop file/URL      play from launcher
  Space, right click  pause/play
  m                  mute/unmute
  v                  subtitles on/off
  i                  show media information
  I                  pin/unpin media information
  Left, Right         seek backward/forward by 5 seconds
  Down, Up            seek backward/forward by 60 seconds
  q                  quit
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle::sidecar_subtitle_path;

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
        assert!(config.resume_enabled);
        assert!(!config.clear_resume);
    }

    #[test]
    fn parse_args_supports_resume_controls() {
        let no_resume = parse_args(vec![OsString::from("--no-resume")].into_iter())
            .expect("--no-resume should parse");
        assert!(!no_resume.resume_enabled);
        assert!(!no_resume.clear_resume);

        let clear = parse_args(vec![OsString::from("--clear-resume")].into_iter())
            .expect("--clear-resume should parse");
        assert!(clear.clear_resume);
        assert!(clear.path.is_none());
    }

    #[test]
    fn launcher_drop_uses_same_media_and_sidecar_path_as_argument() {
        let temp_dir = std::env::temp_dir().join(format!(
            "enzo-app-drop-subtitle-test-{}",
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
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-app-subtitle-test-{}", std::process::id()));
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
}
