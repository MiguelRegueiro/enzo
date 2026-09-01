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

pub(crate) use startup::play;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackOptions {
    pub(crate) resume_enabled: bool,
    pub(crate) autoplay_next: bool,
    pub(crate) volume_max: u16,
    pub(crate) accent_color: [u8; 3],
    pub(crate) force_media_title: Option<std::sync::Arc<str>>,
}
