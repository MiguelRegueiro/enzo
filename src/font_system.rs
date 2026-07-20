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
mod tests {
    use super::*;
    use std::{
        fs::File,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn resolver_prefers_noto_over_other_fonts() {
        let root = temp_font_dir("prefers_noto");
        let random = root.join("Random-Regular.ttf");
        let noto = root.join("NotoSans-Regular.ttf");
        File::create(&random).expect("create random font");
        File::create(&noto).expect("create noto font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all(FontRole::Ui).next(),
            Some(noto.as_path())
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resolver_falls_back_to_first_discovered_font() {
        let root = temp_font_dir("fallback_first");
        let zed = root.join("Zed-Regular.ttf");
        let alpha = root.join("Alpha-Regular.ttf");
        File::create(&zed).expect("create zed font");
        File::create(&alpha).expect("create alpha font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all(FontRole::Subtitle).next(),
            Some(alpha.as_path())
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn japanese_subtitles_prefer_japanese_cjk_fonts() {
        let root = temp_font_dir("japanese_subtitle_font");
        let latin = root.join("NotoSans-Regular.ttf");
        let chinese = root.join("NotoSansCJKSC-Regular.otf");
        let japanese = root.join("noto-cjk/NotoSansCJK-Regular.ttc");
        fs::create_dir_all(japanese.parent().expect("japanese parent"))
            .expect("create japanese dir");
        File::create(&latin).expect("create latin font");
        File::create(&chinese).expect("create chinese font");
        File::create(&japanese).expect("create japanese font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all_for_language(FontRole::Subtitle, Some("ja"))[0],
            japanese
        );
        assert_eq!(
            system.resolve_all_for_language(FontRole::Ui, Some("ja"))[0],
            latin
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn chinese_subtitles_prefer_chinese_cjk_fonts() {
        let root = temp_font_dir("chinese_subtitle_font");
        let latin = root.join("NotoSans-Regular.ttf");
        let chinese = root.join("wenquanyi/wqy-zenhei.ttc");
        fs::create_dir_all(chinese.parent().expect("chinese parent")).expect("create chinese dir");
        File::create(&latin).expect("create latin font");
        File::create(&chinese).expect("create chinese font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all_for_language(FontRole::Subtitle, Some("zh-Hans"))[0],
            chinese
        );
        assert_eq!(
            system.resolve_all_for_language(FontRole::Ui, Some("zh-Hans"))[0],
            latin
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn arabic_subtitles_prefer_arabic_fonts() {
        let root = temp_font_dir("arabic_subtitle_font");
        let latin = root.join("NotoSans-Regular.ttf");
        let arabic = root.join("NotoSansArabic-Regular.ttf");
        File::create(&latin).expect("create latin font");
        File::create(&arabic).expect("create arabic font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all_for_language(FontRole::Subtitle, Some("ar"))[0],
            arabic
        );
        assert_eq!(
            system.resolve_all_for_language(FontRole::Ui, Some("ar"))[0],
            latin
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ui_prefers_plain_fedora_noto_variable_font_over_script_specific_noto_fonts() {
        let root = temp_font_dir("fedora_noto_variable_font");
        let cjk = root
            .join("google-noto-sans-cjk-vf-fonts")
            .join("NotoSansCJK-VF.ttc");
        let arabic = root.join("google-noto-vf").join("NotoSansArabic[wght].ttf");
        let plain = root.join("google-noto-vf").join("NotoSans[wght].ttf");
        fs::create_dir_all(cjk.parent().expect("cjk parent")).expect("create cjk dir");
        fs::create_dir_all(plain.parent().expect("plain parent")).expect("create plain dir");
        File::create(&cjk).expect("create cjk font");
        File::create(&arabic).expect("create arabic font");
        File::create(&plain).expect("create plain font");

        let system = FontSystem::from_dirs([root.clone().into_os_string()]);

        assert_eq!(
            system.resolve_all(FontRole::Ui).next(),
            Some(plain.as_path())
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn accepts_common_unix_font_extensions() {
        assert!(is_font_file(Path::new("font.ttf")));
        assert!(is_font_file(Path::new("font.otf")));
        assert!(is_font_file(Path::new("font.ttc")));
        assert!(!is_font_file(Path::new("font.txt")));
    }

    #[test]
    fn system_dirs_include_freebsd_font_location() {
        assert!(SYSTEM_FONT_DIRS.contains(&"/usr/local/share/fonts"));
    }

    fn temp_font_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = env::temp_dir().join(format!("enzo-font-system-{name}-{nonce}"));
        fs::create_dir_all(&dir).expect("create temp font dir");
        dir
    }
}
