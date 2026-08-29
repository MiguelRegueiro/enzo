//! Shared overlay palette.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OverlayPalette {
    pub(super) accent: [u8; 3],
}

impl OverlayPalette {
    pub(super) fn new(accent: [u8; 3]) -> Self {
        Self { accent }
    }
}

pub(super) const PANEL_COLOR: [u8; 3] = [18, 18, 22];
pub(super) const TRACK_COLOR: [u8; 3] = [82, 82, 91];
pub(super) const TEXT_COLOR: [u8; 3] = [250, 250, 250];
pub(super) const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
