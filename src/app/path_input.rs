use std::path::PathBuf;

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
    let Some((scheme, _)) = text.split_once("://") else {
        return false;
    };
    !scheme.eq_ignore_ascii_case("file") && is_url_scheme(scheme)
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

fn is_url_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
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
}
