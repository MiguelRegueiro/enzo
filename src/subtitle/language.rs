use std::path::Path;

use super::parser::strip_srt_markup;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LanguageName {
    pub(crate) tag: &'static str,
    pub(crate) name: &'static str,
}

const LANGUAGE_NAMES: &[LanguageName] = &[
    LanguageName {
        tag: "ar",
        name: "Arabic",
    },
    LanguageName {
        tag: "bg",
        name: "Bulgarian",
    },
    LanguageName {
        tag: "ca",
        name: "Catalan",
    },
    LanguageName {
        tag: "cs",
        name: "Czech",
    },
    LanguageName {
        tag: "da",
        name: "Danish",
    },
    LanguageName {
        tag: "de",
        name: "German",
    },
    LanguageName {
        tag: "el",
        name: "Greek",
    },
    LanguageName {
        tag: "en",
        name: "English",
    },
    LanguageName {
        tag: "es",
        name: "Spanish",
    },
    LanguageName {
        tag: "et",
        name: "Estonian",
    },
    LanguageName {
        tag: "eu",
        name: "Basque",
    },
    LanguageName {
        tag: "fa",
        name: "Persian",
    },
    LanguageName {
        tag: "fi",
        name: "Finnish",
    },
    LanguageName {
        tag: "fil",
        name: "Filipino",
    },
    LanguageName {
        tag: "fr",
        name: "French",
    },
    LanguageName {
        tag: "he",
        name: "Hebrew",
    },
    LanguageName {
        tag: "hi",
        name: "Hindi",
    },
    LanguageName {
        tag: "hr",
        name: "Croatian",
    },
    LanguageName {
        tag: "hu",
        name: "Hungarian",
    },
    LanguageName {
        tag: "id",
        name: "Indonesian",
    },
    LanguageName {
        tag: "is",
        name: "Icelandic",
    },
    LanguageName {
        tag: "it",
        name: "Italian",
    },
    LanguageName {
        tag: "ja",
        name: "Japanese",
    },
    LanguageName {
        tag: "ko",
        name: "Korean",
    },
    LanguageName {
        tag: "lt",
        name: "Lithuanian",
    },
    LanguageName {
        tag: "lv",
        name: "Latvian",
    },
    LanguageName {
        tag: "ms",
        name: "Malay",
    },
    LanguageName {
        tag: "nb",
        name: "Norwegian Bokmål",
    },
    LanguageName {
        tag: "nl",
        name: "Dutch",
    },
    LanguageName {
        tag: "pl",
        name: "Polish",
    },
    LanguageName {
        tag: "pt",
        name: "Portuguese",
    },
    LanguageName {
        tag: "ro",
        name: "Romanian",
    },
    LanguageName {
        tag: "ru",
        name: "Russian",
    },
    LanguageName {
        tag: "sk",
        name: "Slovak",
    },
    LanguageName {
        tag: "sl",
        name: "Slovenian",
    },
    LanguageName {
        tag: "sr",
        name: "Serbian",
    },
    LanguageName {
        tag: "sv",
        name: "Swedish",
    },
    LanguageName {
        tag: "th",
        name: "Thai",
    },
    LanguageName {
        tag: "tr",
        name: "Turkish",
    },
    LanguageName {
        tag: "uk",
        name: "Ukrainian",
    },
    LanguageName {
        tag: "vi",
        name: "Vietnamese",
    },
    LanguageName {
        tag: "zh",
        name: "Chinese",
    },
    LanguageName {
        tag: "zh-Hans",
        name: "Chinese Simplified",
    },
    LanguageName {
        tag: "zh-Hant",
        name: "Chinese Traditional",
    },
];

pub(crate) fn normalize_language_tag(tag: &str) -> Option<String> {
    let tag = tag.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_');
    if tag.is_empty() {
        return None;
    }
    let lower = tag.replace('_', "-").to_ascii_lowercase();
    if matches!(lower.as_str(), "und" | "unknown" | "none" | "n/a") {
        return None;
    }
    if let Some(mapped) = legacy_language_tag(&lower) {
        return Some(mapped.to_string());
    }
    normalize_bcp47_like_tag(&lower)
}

pub(crate) fn language_name(tag: &str) -> Option<&'static str> {
    LANGUAGE_NAMES
        .iter()
        .find_map(|language| (language.tag == tag).then_some(language.name))
}

pub(crate) fn language_display_name(tag: &str) -> String {
    language_name(tag).unwrap_or(tag).to_string()
}

pub(crate) fn subtitle_codec_label(codec: &str) -> String {
    match codec.trim().to_ascii_lowercase().as_str() {
        "ass" => "ASS".to_string(),
        "ssa" => "SSA".to_string(),
        "subrip" | "srt" => "SRT".to_string(),
        "text" => "Text".to_string(),
        "mov_text" => "MOV text".to_string(),
        "webvtt" => "WebVTT".to_string(),
        "hdmv_pgs_subtitle" => "PGS".to_string(),
        "hdmv_text_subtitle" => "HDMV text".to_string(),
        other => other.to_uppercase(),
    }
}

fn legacy_language_tag(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "alb" | "sqi" => "sq",
        "ara" => "ar",
        "baq" | "eus" => "eu",
        "bul" => "bg",
        "cat" => "ca",
        "chi" | "zho" | "cn" => "zh",
        "chi-hans" | "zho-hans" | "zh-cn" | "zh-sg" | "sc" | "chs" => "zh-Hans",
        "chi-hant" | "zho-hant" | "zh-tw" | "zh-hk" | "zh-mo" | "tc" | "cht" => "zh-Hant",
        "cze" | "ces" => "cs",
        "dan" => "da",
        "dut" | "nld" => "nl",
        "eng" => "en",
        "est" => "et",
        "fil" | "tgl" | "tl" => "fil",
        "fin" => "fi",
        "fre" | "fra" => "fr",
        "ger" | "deu" => "de",
        "gre" | "ell" => "el",
        "heb" | "iw" => "he",
        "hin" => "hi",
        "hrv" => "hr",
        "hun" => "hu",
        "ice" | "isl" => "is",
        "ind" => "id",
        "ita" => "it",
        "jpn" | "jp" => "ja",
        "kor" => "ko",
        "lav" => "lv",
        "lit" => "lt",
        "may" | "msa" => "ms",
        "nob" | "no-bok" => "nb",
        "per" | "fas" => "fa",
        "pol" => "pl",
        "por" => "pt",
        "rum" | "ron" => "ro",
        "rus" => "ru",
        "slo" | "slk" => "sk",
        "slv" => "sl",
        "srp" => "sr",
        "spa" => "es",
        "swe" => "sv",
        "tha" => "th",
        "tur" => "tr",
        "ukr" => "uk",
        "vie" => "vi",
        _ => return None,
    })
}

fn normalize_bcp47_like_tag(tag: &str) -> Option<String> {
    let mut parts = tag.split('-').filter(|part| !part.is_empty());
    let language = parts.next()?;
    if !language.chars().all(|ch| ch.is_ascii_alphanumeric()) || !(2..=8).contains(&language.len())
    {
        return None;
    }

    let mut normalized = vec![language.to_ascii_lowercase()];
    for part in parts {
        if !part.chars().all(|ch| ch.is_ascii_alphanumeric()) || part.len() > 8 {
            return None;
        }
        let value = match part.len() {
            2 if part.chars().all(|ch| ch.is_ascii_alphabetic()) => part.to_ascii_uppercase(),
            4 if part.chars().all(|ch| ch.is_ascii_alphabetic()) => titlecase_ascii(part),
            _ => part.to_ascii_lowercase(),
        };
        normalized.push(value);
    }
    Some(normalized.join("-"))
}

fn titlecase_ascii(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_ascii_uppercase().to_string();
    out.push_str(&chars.as_str().to_ascii_lowercase());
    out
}

#[cfg(test)]
#[path = "tests/language.rs"]
mod tests;

pub(super) fn infer_subtitle_language(path: &Path, text: &str) -> Option<String> {
    language_from_filename(path).or_else(|| detect_text_language(text))
}

pub(super) fn language_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let normalized = stem.replace(['_', ' ', '[', ']', '(', ')'], ".");
    let parts = normalized
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    for pair in parts.windows(2) {
        let candidate = format!("{}-{}", pair[0], pair[1]);
        if let Some(language) = normalize_filename_language_tag(&candidate) {
            return Some(language);
        }
    }

    for part in &parts {
        if let Some(language) = normalize_filename_language_tag(part) {
            return Some(language);
        }
    }

    None
}

pub(super) fn normalize_filename_language_tag(tag: &str) -> Option<String> {
    let language = normalize_language_tag(tag)?;
    language_name(&language).is_some().then_some(language)
}

pub(super) fn detect_text_language(text: &str) -> Option<String> {
    let mut cjk = 0_u32;
    let mut kana = 0_u32;
    let mut hangul = 0_u32;
    let mut cyrillic = 0_u32;
    let mut arabic = 0_u32;
    let mut latin = 0_u32;
    let mut english_stopwords = 0_u32;

    for line in text.lines().take(400) {
        let line = strip_srt_markup(line);
        if line.contains("-->") || line.trim().parse::<u32>().is_ok() {
            continue;
        }

        for ch in line.chars() {
            match ch {
                '\u{3040}'..='\u{30ff}' => kana += 1,
                '\u{3400}'..='\u{9fff}' => cjk += 1,
                '\u{ac00}'..='\u{d7af}' => hangul += 1,
                '\u{0400}'..='\u{04ff}' => cyrillic += 1,
                '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{0870}'..='\u{08ff}' => {
                    arabic += 1
                }
                ch if ch.is_ascii_alphabetic() => latin += 1,
                _ => {}
            }
        }

        for word in line
            .split(|ch: char| !ch.is_ascii_alphabetic() && ch != '\'')
            .map(|word| word.to_ascii_lowercase())
        {
            if is_english_stopword(&word) {
                english_stopwords += 1;
            }
        }
    }

    if kana >= 4 {
        return Some("ja".to_string());
    }
    if hangul >= 4 {
        return Some("ko".to_string());
    }
    if cjk >= 4 {
        return Some("zh".to_string());
    }
    if cyrillic >= 12 {
        return Some("ru".to_string());
    }
    if arabic >= 4 {
        return Some("ar".to_string());
    }
    if latin >= 40 && english_stopwords >= 4 {
        return Some("en".to_string());
    }

    None
}

pub(super) fn is_english_stopword(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "but"
            | "by"
            | "for"
            | "from"
            | "had"
            | "have"
            | "he"
            | "i"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "not"
            | "of"
            | "on"
            | "or"
            | "she"
            | "that"
            | "the"
            | "they"
            | "this"
            | "to"
            | "was"
            | "we"
            | "were"
            | "with"
            | "you"
            | "your"
    )
}
