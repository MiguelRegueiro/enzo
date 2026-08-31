use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::source_parsing::media_candidates_from_text;

pub(crate) fn is_remote_url_text(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

pub(crate) fn media_path_from_argument(path: PathBuf) -> Result<PathBuf> {
    let text = path.as_os_str().to_string_lossy();
    let path = media_candidates_from_text(&text)
        .into_iter()
        .next()
        .unwrap_or(path);
    validate_media_path(&path)?;
    Ok(path)
}

pub(crate) fn media_path_from_drop_text(text: &str) -> Result<PathBuf> {
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

pub(crate) fn validate_subtitle_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("subtitle file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("subtitle path is not a file: {}", path.display());
    }
    Ok(())
}

fn validate_media_path(path: &Path) -> Result<()> {
    let text = path.as_os_str().to_string_lossy();
    if is_remote_url_text(&text) {
        return Ok(());
    }
    if path.exists() {
        if !path.is_file() {
            bail!("video path is not a file: {}", path.display());
        }
        return Ok(());
    }
    if let Some(scheme) = url_scheme(&text) {
        bail!(
            "unsupported media URL scheme `{scheme}`; only http:// and https:// URLs are supported"
        );
    }
    bail!(
        "video does not exist: {}. If the path contains spaces, quote it.",
        path.display()
    )
}

fn url_scheme(text: &str) -> Option<&str> {
    let (scheme, _) = text.split_once(':')?;
    let mut chars = scheme.chars();
    chars.next()?.is_ascii_alphabetic().then_some(())?;
    chars
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        .then_some(scheme)
}

#[cfg(test)]
#[path = "tests/source_resolution.rs"]
mod tests;
