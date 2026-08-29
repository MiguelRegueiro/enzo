use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use serde::Deserialize;

pub(crate) const MIN_VOLUME_MAX: u16 = 100;
pub(crate) const MAX_VOLUME_MAX: u16 = 1000;
pub(crate) const DEFAULT_ACCENT_COLOR: [u8; 3] = [239, 68, 68];
const DEFAULT_VOLUME_MAX: u16 = MIN_VOLUME_MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Config {
    pub(crate) volume_max: u16,
    pub(crate) resume: bool,
    pub(crate) autoplay_next: bool,
    pub(crate) accent_color: [u8; 3],
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    volume_max: Option<u16>,
    resume: Option<bool>,
    autoplay_next: Option<bool>,
    accent_color: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume_max: DEFAULT_VOLUME_MAX,
            resume: true,
            autoplay_next: true,
            accent_color: DEFAULT_ACCENT_COLOR,
        }
    }
}

impl Config {
    pub(crate) fn load(override_path: Option<&Path>) -> Result<Self> {
        let is_override = override_path.is_some();
        let Some(path) = override_path.map(Path::to_path_buf).or_else(config_path) else {
            return Ok(Self::default());
        };
        load_from_path(&path, is_override)
    }

    fn from_str(contents: &str) -> Result<Self> {
        let parsed = toml::from_str::<ConfigFile>(contents)?;
        let volume_max = parsed.volume_max.unwrap_or(DEFAULT_VOLUME_MAX);
        if !(MIN_VOLUME_MAX..=MAX_VOLUME_MAX).contains(&volume_max) {
            bail!("volume_max must be between {MIN_VOLUME_MAX} and {MAX_VOLUME_MAX}");
        }
        Ok(Self {
            volume_max,
            resume: parsed.resume.unwrap_or(true),
            autoplay_next: parsed.autoplay_next.unwrap_or(true),
            accent_color: parsed
                .accent_color
                .as_deref()
                .map(parse_hex_color)
                .transpose()?
                .unwrap_or(DEFAULT_ACCENT_COLOR),
        })
    }
}

fn parse_hex_color(value: &str) -> Result<[u8; 3]> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        bail!("accent_color must use #RRGGBB format");
    }
    let mut color = [0_u8; 3];
    for (component, pair) in color.iter_mut().zip(bytes[1..].chunks_exact(2)) {
        let Some(high) = hex_digit(pair[0]) else {
            bail!("accent_color must use #RRGGBB format");
        };
        let Some(low) = hex_digit(pair[1]) else {
            bail!("accent_color must use #RRGGBB format");
        };
        *component = high * 16 + low;
    }
    Ok(color)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn load_from_path(path: &Path, is_override: bool) -> Result<Config> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if !is_override && error.kind() == io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) if is_override => {
            bail!("failed to read config from {}: {error}", path.display());
        }
        Err(error) => {
            eprintln!(
                "enzo: failed to read config from {}: {error}",
                path.display()
            );
            return Ok(Config::default());
        }
    };

    Ok(match Config::from_str(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "enzo: failed to load config from {}: {error}",
                path.display()
            );
            Config::default()
        }
    })
}

fn config_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        platform_config_home(
            env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new),
            dirs::home_dir().as_deref(),
        )
    }

    #[cfg(windows)]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
    }

    #[cfg(not(any(unix, windows)))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
    }
}

#[cfg(unix)]
fn platform_config_home(xdg_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg_home) = xdg_home {
        return Some(xdg_home.to_path_buf());
    }
    let home = home?;

    #[cfg(target_os = "macos")]
    {
        let xdg_home = home.join(".config");
        if xdg_home.join("enzo/config.toml").is_file() {
            return Some(xdg_home);
        }
        return Some(home.join("Library/Application Support"));
    }

    #[cfg(not(target_os = "macos"))]
    Some(home.join(".config"))
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    config_home().map(|home| home.join("enzo"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the Unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("enzo-config-{label}-{unique}"))
    }

    #[test]
    fn defaults_match_existing_playback_behavior() {
        assert_eq!(
            Config::default(),
            Config {
                volume_max: 100,
                resume: true,
                autoplay_next: true,
                accent_color: DEFAULT_ACCENT_COLOR,
            }
        );
    }

    #[test]
    fn partial_config_overrides_defaults() {
        let config =
            Config::from_str("volume_max = 220\nresume = false\naccent_color = \"#1aB2c3\"\n")
                .expect("valid config should parse");

        assert_eq!(config.volume_max, 220);
        assert!(!config.resume);
        assert!(config.autoplay_next);
        assert_eq!(config.accent_color, [0x1a, 0xb2, 0xc3]);
    }

    #[test]
    fn config_rejects_invalid_values_and_unknown_keys() {
        for contents in [
            "volume_max = 99\n",
            "volume_max = 1001\n",
            "resum = true\n",
            "accent_color = \"ef4444\"\n",
            "accent_color = \"#abcd\"\n",
            "accent_color = \"#gg0000\"\n",
            "accent_color = #ef4444\n",
        ] {
            assert!(Config::from_str(contents).is_err(), "contents: {contents}");
        }
    }

    #[test]
    fn explicit_config_path_is_loaded() {
        let root = temp_path("explicit");
        let path = root.join("custom.toml");
        fs::create_dir_all(&root).expect("config directory should be created");
        fs::write(
            &path,
            "volume_max = 180\nresume = false\nautoplay_next = false\naccent_color = \"#102030\"\n",
        )
        .expect("config should be written");

        let config = Config::load(Some(&path)).expect("explicit config should load");

        assert_eq!(
            config,
            Config {
                volume_max: 180,
                resume: false,
                autoplay_next: false,
                accent_color: [0x10, 0x20, 0x30],
            }
        );
        fs::remove_dir_all(root).expect("config directory should be removed");
    }

    #[test]
    fn missing_explicit_config_path_is_an_error() {
        let path = temp_path("missing").join("config.toml");

        let error = Config::load(Some(&path)).expect_err("missing explicit config should fail");

        assert!(
            error
                .to_string()
                .contains(&format!("failed to read config from {}", path.display()))
        );
    }

    #[test]
    fn invalid_config_falls_back_to_defaults() {
        let root = temp_path("invalid");
        let path = root.join("config.toml");
        fs::create_dir_all(&root).expect("config directory should be created");
        fs::write(&path, "volume_max = 20\n").expect("config should be written");

        let config = Config::load(Some(&path)).expect("invalid config should fall back");

        assert_eq!(config, Config::default());
        fs::remove_dir_all(root).expect("config directory should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn xdg_config_home_takes_precedence() {
        assert_eq!(
            platform_config_home(
                Some(Path::new("/tmp/custom-config")),
                Some(Path::new("/home/paco")),
            ),
            Some(PathBuf::from("/tmp/custom-config"))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn unix_config_home_defaults_to_dot_config() {
        assert_eq!(
            platform_config_home(None, Some(Path::new("/home/paco"))),
            Some(PathBuf::from("/home/paco/.config"))
        );
    }
}
