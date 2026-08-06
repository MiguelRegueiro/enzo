use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

pub(crate) fn media_candidates_from_text(text: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        push_candidate(&mut candidates, line);
        for token in shell_words(line) {
            push_candidate(&mut candidates, &token);
        }
    }

    if candidates.is_empty() {
        let text = text.trim();
        if !text.is_empty() {
            push_candidate(&mut candidates, text);
        }
    }

    dedupe_candidates(candidates)
}

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

fn push_candidate(candidates: &mut Vec<PathBuf>, text: &str) {
    if let Some(candidate) = parse_candidate(text) {
        candidates.push(candidate);
    }
}

fn parse_candidate(text: &str) -> Option<PathBuf> {
    let text = strip_wrapping_quotes(text.trim());
    if text.is_empty() {
        return None;
    }

    if let Some(path) = file_url_path(text) {
        return Some(path);
    }
    if is_remote_url_text(text) {
        return Some(PathBuf::from(text));
    }

    Some(PathBuf::from(unescape_backslashes(text)))
}

fn file_url_path(text: &str) -> Option<PathBuf> {
    let rest = text.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(percent_decode(rest)))
}

fn strip_wrapping_quotes(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        &text[1..text.len() - 1]
    } else {
        text
    }
}

fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    let mut quote = None::<char>;

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\'') | (None, '"') => quote = Some(ch),
            (Some(q), c) if q == c => quote = None,
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (Some('\''), c) => current.push(c),
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, c) => current.push(c),
        }
    }

    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn unescape_backslashes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn url_scheme(text: &str) -> Option<&str> {
    let (scheme, _) = text.split_once(':')?;
    let mut chars = scheme.chars();
    chars.next()?.is_ascii_alphabetic().then_some(())?;
    chars
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        .then_some(scheme)
}

fn dedupe_candidates(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for candidate in candidates {
        if !deduped.contains(&candidate) {
            deduped.push(candidate);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle::sidecar_subtitle_paths;

    #[test]
    fn argument_and_drop_resolve_the_same_media_and_sidecar_paths() {
        let temp_dir =
            std::env::temp_dir().join(format!("enzo-media-input-test-{}", std::process::id()));
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
        assert_eq!(
            sidecar_subtitle_paths(&from_arg),
            std::slice::from_ref(&sidecar)
        );
        assert_eq!(
            sidecar_subtitle_paths(&from_drop),
            std::slice::from_ref(&sidecar)
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parses_plain_path_drop() {
        assert_eq!(
            media_candidates_from_text("/tmp/video.mp4").first(),
            Some(&PathBuf::from("/tmp/video.mp4"))
        );
    }

    #[test]
    fn parses_shell_escaped_path_drop() {
        assert!(
            media_candidates_from_text("/tmp/video\\ file.mp4")
                .contains(&PathBuf::from("/tmp/video file.mp4"))
        );
    }

    #[test]
    fn parses_file_url_drop() {
        assert_eq!(
            media_candidates_from_text("file:///tmp/video%20file.mp4").first(),
            Some(&PathBuf::from("/tmp/video file.mp4"))
        );
    }

    #[test]
    fn keeps_remote_url_drop() {
        assert_eq!(
            media_candidates_from_text("https://example.com/video.mp4").first(),
            Some(&PathBuf::from("https://example.com/video.mp4"))
        );
    }

    #[test]
    fn only_http_and_https_are_remote_media_urls() {
        assert!(is_remote_url_text("http://example.com/video.mp4"));
        assert!(is_remote_url_text("https://example.com/video.mp4"));
        assert!(!is_remote_url_text("ftp://example.com/video.mp4"));
        assert!(!is_remote_url_text("concat:/tmp/one.ts|/tmp/two.ts"));
        assert!(!is_remote_url_text("file:///tmp/video.mp4"));
    }

    #[test]
    fn rejects_unsupported_media_protocols() {
        for input in [
            "ftp://example.com/video.mp4",
            "concat:/tmp/one.ts|/tmp/two.ts",
            "subfile:/tmp/video.mp4",
            "lavfi:testsrc",
        ] {
            let error = media_path_from_argument(PathBuf::from(input))
                .expect_err("unsafe protocol should be rejected");
            assert!(
                error.to_string().contains("unsupported media URL scheme"),
                "unexpected error for {input}: {error}"
            );
        }
    }

    #[test]
    fn existing_local_filename_with_colon_remains_valid() {
        let path = std::env::temp_dir().join(format!(
            "enzo-media-input-colon-test-{}:video.mkv",
            std::process::id()
        ));
        std::fs::write(&path, "video").expect("test file should be written");

        assert_eq!(
            media_path_from_argument(path.clone()).expect("existing file should be accepted"),
            path
        );

        let _ = std::fs::remove_file(path);
    }
}
