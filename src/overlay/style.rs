//! Shared overlay palette.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OverlayPalette {
    pub(super) accent: [u8; 3],
    pub(super) panel_alpha: u8,
    pub(super) blur_radius: u32,
}

impl OverlayPalette {
    pub(super) fn new(accent: [u8; 3], panel_opacity: u8, background_blur: u8) -> Self {
        Self {
            accent,
            panel_alpha: (u16::from(panel_opacity) * 255 / 100) as u8,
            blur_radius: u32::from(background_blur),
        }
    }
}

pub(super) const PANEL_COLOR: [u8; 3] = [18, 18, 22];
pub(super) const TRACK_COLOR: [u8; 3] = [82, 82, 91];
pub(super) const TEXT_COLOR: [u8; 3] = [250, 250, 250];
pub(super) const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
