#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PlaybackCarryover {
    pub(super) paused: bool,
    pub(super) muted: bool,
    pub(super) volume_percent: u16,
    pub(super) volume_max: u16,
    pub(super) media_info_pinned: bool,
}

impl PlaybackCarryover {
    pub(super) fn new(volume_max: u16) -> Self {
        Self {
            paused: false,
            muted: false,
            volume_percent: 100,
            volume_max,
            media_info_pinned: false,
        }
    }
}
