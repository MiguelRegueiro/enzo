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
    pub(super) fn from_str(contents: &str) -> Result<Self> {
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

#[cfg(test)]
#[path = "tests/config_definition.rs"]
mod tests;
