use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontRole {
    Ui,
    Subtitle,
}

#[derive(Debug)]
pub(crate) struct FontSystem {
    fonts: Vec<FontCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FontCandidate {
    path: PathBuf,
    family_hint: String,
}

const SYSTEM_FONT_DIRS: &[&str] = &[
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "~/.local/share/fonts",
    "~/.fonts",
];

const PREFERRED_FONT_PATTERNS: &[&str] = &[
    "notosans[wght]",
    "notosans-regular",
    "notosans",
    "opensans-regular",
    "opensans",
    "adwaitasans-regular",
    "adwaitasans",
    "dejavusans",
    "vera",
];

const JAPANESE_SUBTITLE_FONT_PATTERNS: &[&str] = &[
    "notosanscjkjp",
    "noto-cjk",
    "notosanscjk",
    "sourcehansansjp",
    "sourcehansans",
    "ipaexgothic",
    "ipagothic",
    "bizud",
    "takao",
    "hiragino",
    "yugothic",
    "japanese",
    "japan",
];

const CHINESE_SUBTITLE_FONT_PATTERNS: &[&str] = &[
    "notosanscjksc",
    "sourcehansanssc",
    "wqyzenhei",
    "wenquanyi",
    "microsoftyahei",
    "yahei",
    "simhei",
    "simsun",
    "kaiti",
    "notosanscjk",
    "noto-cjk",
    "sourcehansans",
    "chinese",
];

const ARABIC_SUBTITLE_FONT_PATTERNS: &[&str] = &[
    "notosansarabic-regular",
    "notosansarabic",
    "notonaskharabic-regular",
    "notonaskharabic",
    "notokufiarabic",
    "arabic",
    "dejavusans",
];

impl FontSystem {
    pub(crate) fn discover() -> Self {
        Self::from_dirs(SYSTEM_FONT_DIRS.iter().map(OsString::from))
    }

    pub(crate) fn resolve_all(&self, role: FontRole) -> impl Iterator<Item = &Path> + '_ {
        let mut fonts = self.fonts.iter().collect::<Vec<_>>();
        fonts.sort_by_key(|font| font.preference_rank(role, None));
        fonts.into_iter().map(|font| font.path.as_path())
    }

    pub(crate) fn resolve_all_for_language(
        &self,
        role: FontRole,
        language: Option<&str>,
    ) -> Vec<PathBuf> {
        let mut fonts = self.fonts.iter().collect::<Vec<_>>();
        fonts.sort_by_key(|font| font.preference_rank(role, language));
        fonts
            .into_iter()
            .map(|font| font.path.clone())
            .collect::<Vec<_>>()
    }

    fn from_dirs(dirs: impl IntoIterator<Item = OsString>) -> Self {
        let mut fonts = Vec::new();
        for dir in dirs {
            let Some(dir) = expand_home(dir) else {
                continue;
            };
            collect_font_candidates(&dir, &mut fonts);
        }
        fonts.sort_by(|a, b| a.path.cmp(&b.path));
        fonts.dedup_by(|a, b| a.path == b.path);
        Self { fonts }
    }
}

impl FontCandidate {
    fn from_path(path: PathBuf) -> Option<Self> {
        if !is_font_file(&path) {
            return None;
        }
        let family_hint = path
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace([' ', '_'], "");
        Some(Self { path, family_hint })
    }

    fn preference_rank(&self, role: FontRole, language: Option<&str>) -> (usize, usize, &Path) {
        let language_patterns = language
            .filter(|_| role == FontRole::Subtitle)
            .and_then(subtitle_font_patterns);
        let language_rank = language_patterns.map_or(0, |patterns| {
            pattern_rank(&self.family_hint, patterns).unwrap_or(patterns.len())
        });
        let pattern_rank = pattern_rank(&self.family_hint, PREFERRED_FONT_PATTERNS)
            .unwrap_or(PREFERRED_FONT_PATTERNS.len());
        (language_rank, pattern_rank, self.path.as_path())
    }
}

fn pattern_rank(haystack: &str, patterns: &[&str]) -> Option<usize> {
    patterns
        .iter()
        .position(|pattern| haystack.contains(pattern))
}

fn is_japanese_language(language: &str) -> bool {
    let language = language.to_ascii_lowercase();
    language == "ja" || language == "jpn" || language.starts_with("ja-")
}

fn subtitle_font_patterns(language: &str) -> Option<&'static [&'static str]> {
    let language = language.to_ascii_lowercase();
    if is_japanese_language(&language) {
        Some(JAPANESE_SUBTITLE_FONT_PATTERNS)
    } else if matches!(language.as_str(), "zh" | "chi" | "zho") || language.starts_with("zh-") {
        Some(CHINESE_SUBTITLE_FONT_PATTERNS)
    } else if matches!(language.as_str(), "ar" | "ara") || language.starts_with("ar-") {
        Some(ARABIC_SUBTITLE_FONT_PATTERNS)
    } else {
        None
    }
}

fn collect_font_candidates(dir: &Path, fonts: &mut Vec<FontCandidate>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_candidates(&path, fonts);
        } else if let Some(candidate) = FontCandidate::from_path(path) {
            fonts.push(candidate);
        }
    }
}

fn expand_home(path: OsString) -> Option<PathBuf> {
    let text = path.to_string_lossy();
    if text == "~" {
        return home_dir();
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return home_dir().map(|home| home.join(rest));
    }
    Some(PathBuf::from(path))
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc"
            )
        })
}

#[cfg(test)]
#[path = "tests/font_catalog.rs"]
mod tests;
