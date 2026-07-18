//! Playback orchestration and its internal component boundaries.
//!
//! `startup` assembles a session, `session` owns its lifecycle and tick order,
//! and `interaction` translates terminal intent into state changes. The
//! remaining modules own one policy or resource family each.

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
