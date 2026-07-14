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
    "notosans-regular",
    "notosans",
    "opensans-regular",
    "opensans",
    "adwaitasans-regular",
    "adwaitasans",
    "dejavusans",
    "vera",
];

impl FontSystem {
    pub(crate) fn discover() -> Self {
        Self::from_dirs(SYSTEM_FONT_DIRS.iter().map(OsString::from))
    }

    pub(crate) fn resolve_all(&self, _role: FontRole) -> impl Iterator<Item = &Path> + '_ {
        let mut fonts = self.fonts.iter().collect::<Vec<_>>();
        fonts.sort_by_key(|font| font.preference_rank());
        fonts.into_iter().map(|font| font.path.as_path())
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
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .replace([' ', '_'], "");
        Some(Self { path, family_hint })
    }

    fn preference_rank(&self) -> (usize, &Path) {
        let pattern_rank = PREFERRED_FONT_PATTERNS
            .iter()
            .position(|pattern| self.family_hint.contains(pattern))
            .unwrap_or(PREFERRED_FONT_PATTERNS.len());
        (pattern_rank, self.path.as_path())
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
        let dir = env::temp_dir().join(format!("rigoberto-font-system-{name}-{nonce}"));
        fs::create_dir_all(&dir).expect("create temp font dir");
        dir
    }
}
