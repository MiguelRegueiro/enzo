use super::*;
use crate::playback::session::PlaybackOutcome;
use crate::playlist::PlaylistStep;

#[test]
fn completion_autoplays_next_only_when_enabled_and_available() {
    let middle = PlaylistControls {
        previous_available: true,
        next_available: true,
    };
    let last = PlaylistControls {
        previous_available: true,
        next_available: false,
    };

    assert_eq!(
        next_playlist_change(PlaybackOutcome::Completed, middle, true),
        Some(PlaylistChange::Step(PlaylistStep::Next))
    );
    assert_eq!(
        next_playlist_change(PlaybackOutcome::Completed, middle, false),
        None
    );
    assert_eq!(
        next_playlist_change(PlaybackOutcome::Completed, last, true),
        None
    );
}

#[test]
fn manual_playlist_switch_ignores_autoplay_policy() {
    assert_eq!(
        next_playlist_change(
            PlaybackOutcome::Switch(PlaylistStep::Previous),
            PlaylistControls::default(),
            false,
        ),
        Some(PlaylistChange::Step(PlaylistStep::Previous))
    );
    assert_eq!(
        next_playlist_change(
            PlaybackOutcome::SelectPlaylistEntry(7),
            PlaylistControls::default(),
            false,
        ),
        Some(PlaylistChange::Select(7))
    );
}

#[test]
fn forced_media_title_only_applies_to_the_initial_playlist_entry() {
    let initial = Path::new("/videos/Episode 1.mkv");
    let sibling = Path::new("/videos/Episode 2.mkv");

    assert_eq!(
        force_media_title_for_entry(initial, initial, Some("Custom title")),
        Some("Custom title")
    );
    assert_eq!(
        force_media_title_for_entry(sibling, initial, Some("Custom title")),
        None
    );
}
