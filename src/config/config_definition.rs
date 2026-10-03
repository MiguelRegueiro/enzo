use anyhow::{Result, bail};
use serde::Deserialize;

pub(crate) const MIN_VOLUME_MAX: u16 = 100;
pub(crate) const MAX_VOLUME_MAX: u16 = 1000;
pub(crate) const DEFAULT_ACCENT_COLOR: [u8; 3] = [239, 68, 68];
pub(crate) const DEFAULT_PANEL_OPACITY: u8 = 70;
pub(crate) const DEFAULT_BACKGROUND_BLUR: u8 = 12;
pub(crate) const MAX_BACKGROUND_BLUR: u8 = 24;
pub(crate) const DEFAULT_PLAYBACK_CONTROLS_AUTOHIDE: u16 = 2;
pub(crate) const MAX_PLAYBACK_CONTROLS_AUTOHIDE: u16 = 9_999;
const DEFAULT_VOLUME_MAX: u16 = MIN_VOLUME_MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Config {
    pub(crate) volume_max: u16,
    pub(crate) resume: bool,
    pub(crate) autoplay_next: bool,
    pub(crate) accent_color: [u8; 3],
    pub(crate) custom_accent_color: Option<[u8; 3]>,
    pub(crate) panel_opacity: u8,
    pub(crate) background_blur: u8,
    /// Seconds; zero keeps playback controls visible until another action closes them.
    pub(crate) playback_controls_autohide: u16,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    volume_max: Option<u16>,
    resume: Option<bool>,
    autoplay_next: Option<bool>,
    accent_color: Option<String>,
    custom_accent_color: Option<String>,
    panel_opacity: Option<u8>,
    background_blur: Option<u8>,
    playback_controls_autohide: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume_max: DEFAULT_VOLUME_MAX,
            resume: true,
            autoplay_next: true,
            accent_color: DEFAULT_ACCENT_COLOR,
            custom_accent_color: None,
            panel_opacity: DEFAULT_PANEL_OPACITY,
            background_blur: DEFAULT_BACKGROUND_BLUR,
            playback_controls_autohide: DEFAULT_PLAYBACK_CONTROLS_AUTOHIDE,
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
        let panel_opacity = parsed.panel_opacity.unwrap_or(DEFAULT_PANEL_OPACITY);
        if panel_opacity > 100 {
            bail!("panel_opacity must be between 0 and 100");
        }
        let background_blur = parsed.background_blur.unwrap_or(DEFAULT_BACKGROUND_BLUR);
        if background_blur > MAX_BACKGROUND_BLUR {
            bail!("background_blur must be between 0 and {MAX_BACKGROUND_BLUR}");
        }
        let playback_controls_autohide = parsed
            .playback_controls_autohide
            .unwrap_or(DEFAULT_PLAYBACK_CONTROLS_AUTOHIDE);
        if playback_controls_autohide > MAX_PLAYBACK_CONTROLS_AUTOHIDE {
            bail!(
                "playback_controls_autohide must be between 0 and {MAX_PLAYBACK_CONTROLS_AUTOHIDE}"
            );
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
            custom_accent_color: parsed
                .custom_accent_color
                .as_deref()
                .map(parse_hex_color)
                .transpose()?,
            panel_opacity,
            background_blur,
            playback_controls_autohide,
        })
    }
}

pub(crate) fn parse_hex_color(value: &str) -> Result<[u8; 3]> {
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
