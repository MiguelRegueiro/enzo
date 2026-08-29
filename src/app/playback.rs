//! Playback orchestration and its internal component boundaries.
//!
//! `startup` assembles a session, `session` owns its lifecycle and tick order,
//! and `interaction` translates terminal intent into state changes. The
//! remaining modules own one policy or resource family each.

mod carryover;
mod engine;
mod interaction;
mod layout;
mod metadata;
mod pointer;
mod resume_selection;
mod seek;
mod session;
mod startup;
mod subtitles;
mod tracks;
mod ui;
mod view;

pub(super) use startup::play;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlaybackOptions {
    pub(super) resume_enabled: bool,
    pub(super) autoplay_next: bool,
    pub(super) volume_max: u16,
    pub(super) accent_color: [u8; 3],
    pub(super) force_media_title: Option<std::sync::Arc<str>>,
}
