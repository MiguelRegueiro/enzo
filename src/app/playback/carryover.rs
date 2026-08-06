#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PlaybackCarryover {
    pub(super) paused: bool,
    pub(super) muted: bool,
    pub(super) volume_percent: u8,
    pub(super) media_info_pinned: bool,
}

impl Default for PlaybackCarryover {
    fn default() -> Self {
        Self {
            paused: false,
            muted: false,
            volume_percent: 100,
            media_info_pinned: false,
        }
    }
}
