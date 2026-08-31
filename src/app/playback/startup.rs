use std::{
    io::{self, BufWriter},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};

use crate::{
    font::FontSystem,
    media::{AudioPlayer, probe_video},
    overlay::MediaInfo,
    resume::ResumeTracker,
    subtitle::SubtitleTrack,
};

use super::super::playlist::{Playlist, PlaylistControls, PlaylistView};
use super::{
    PlaybackOptions,
    carryover::PlaybackCarryover,
    engine::PlaybackEngine,
    layout::terminal_target_and_canvas,
    metadata::file_info_summary,
    resume_selection::{
        restore_subtitle_selection, resume_available, sync_resume_audio, sync_resume_subtitle,
    },
    seek::{PendingSeek, SeekCoordinator},
    session::{PlaybackSession, PlaybackSessionInit},
    subtitles::{SubtitleCatalog, initial_external_subtitle_paths, load_initial_subtitle_tracks},
    tracks::AudioCatalog,
    ui::{PlaybackUi, resolve_media_title},
    view::PlaybackView,
};

pub(crate) fn play(
    path: PathBuf,
    sub_file: Option<&Path>,
    options: PlaybackOptions,
    font_system: &FontSystem,
) -> Result<()> {
    let mut playlist = Playlist::from_opened_path(path);
    let mut carryover = PlaybackCarryover::new(options.volume_max);
    let initial_path = playlist.current().to_path_buf();
    loop {
        let playlist_view = playlist.view();
        let playlist_controls = playlist_view.controls;
        let entry_sub_file = (playlist.current() == initial_path)
            .then_some(sub_file)
            .flatten();
        let entry_media_title = force_media_title_for_entry(
            playlist.current(),
            &initial_path,
            options.force_media_title.as_deref(),
        );
        let result = play_current(
            playlist.current().to_path_buf(),
            playlist_view,
            carryover,
            entry_sub_file,
            entry_media_title,
            &options,
            font_system,
        )?;
        carryover = result.carryover;
        let Some(change) =
            next_playlist_change(result.outcome, playlist_controls, options.autoplay_next)
        else {
            return Ok(());
        };
        let changed = match change {
            PlaylistChange::Step(step) => playlist.step(step),
            PlaylistChange::Select(index) => playlist.select(index),
        };
        if changed.is_none() {
            return Ok(());
        }
    }
}

fn play_current(
    path: PathBuf,
    playlist: PlaylistView,
    carryover: PlaybackCarryover,
    sub_file: Option<&Path>,
    force_media_title: Option<&str>,
    options: &PlaybackOptions,
    font_system: &FontSystem,
) -> Result<super::session::PlaybackSessionResult> {
    let source = probe_video(&path)
        .with_context(|| format!("could not open media\n  file: {}", path.display()))?;
    let mut resume = ResumeTracker::open(
        &path,
        source.duration,
        resume_available(options.resume_enabled, source.seekable),
        force_media_title,
    );
    let restored = resume.restored().cloned();
    let (initial_subtitle_paths, mut restored_external_subtitle_missing) =
        initial_external_subtitle_paths(&path, sub_file, restored.as_ref());
    let initial_subtitles = load_initial_subtitle_tracks(&path, &initial_subtitle_paths)?;
    restored_external_subtitle_missing |= initial_subtitles.restored_external_load_failed;
    let selected_subtitle = restore_subtitle_selection(
        &initial_subtitles.tracks,
        restored.as_ref(),
        initial_subtitles.restored_external_index,
    )
    .unwrap_or_else(|| (!initial_subtitles.tracks.is_empty()).then_some(0));
    let subtitles = SubtitleCatalog::new(path.clone(), initial_subtitles, selected_subtitle);
    let audio = AudioCatalog::load(&path, source.has_audio, restored.as_ref());
    let media_info = MediaInfo::new(
        file_info_summary(&path, &source),
        source.source_summary(),
        audio.playback_summaries(),
    );
    let (target, canvas) = terminal_target_and_canvas(source.width, source.height);
    let start_position = restored
        .as_ref()
        .and_then(|restored| restored.position)
        .unwrap_or(Duration::ZERO);

    let engine = PlaybackEngine::open(
        &path,
        target,
        source.fps,
        start_position,
        source.has_audio,
        audio.choice(),
        carryover,
    )?;

    resume.set_position(start_position);
    sync_resume_audio(&mut resume, audio.tracks(), audio.selected());
    sync_resume_subtitle(&mut resume, &path, subtitles.tracks(), subtitles.selected());

    let stdout = io::stdout();
    let output =
        BufWriter::with_capacity(canvas.frame_len() + canvas.frame_len() / 2, stdout.lock());
    let view = PlaybackView::new(
        output,
        target,
        canvas,
        font_system,
        subtitles.active().and_then(SubtitleTrack::language),
        options.accent_color,
    )?;
    let status_message = if restored_external_subtitle_missing {
        Some(PlaybackUi::status(
            "SAVED SUBTITLE MISSING",
            engine.started_at,
        ))
    } else {
        resume
            .take_error()
            .map(|_| PlaybackUi::status("RESUME STATE UNAVAILABLE", engine.started_at))
    };
    let ui = PlaybackUi::new(
        resolve_media_title(&path, force_media_title),
        media_info,
        status_message,
        carryover.media_info_pinned,
        playlist.current,
        playlist.labels,
    );
    let seeking = SeekCoordinator::new(PendingSeek {
        video_generation: engine.video.seek_generation(),
        video_target: start_position,
        video_pts: None,
        video_frame_displayed: false,
        audio_generation: engine.audio.as_ref().map(AudioPlayer::seek_generation),
        audio_target: engine.audio.as_ref().map(|_| start_position),
        release_requested: true,
    });

    PlaybackSession::new(PlaybackSessionInit {
        font_system,
        path,
        source,
        playlist_controls: playlist.controls,
        resume,
        audio,
        subtitles,
        engine,
        view,
        ui,
        seeking,
    })
    .run()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaylistChange {
    Step(super::super::playlist::PlaylistStep),
    Select(usize),
}

fn next_playlist_change(
    outcome: super::session::PlaybackOutcome,
    controls: PlaylistControls,
    autoplay_next: bool,
) -> Option<PlaylistChange> {
    match outcome {
        super::session::PlaybackOutcome::Switch(step) => Some(PlaylistChange::Step(step)),
        super::session::PlaybackOutcome::SelectPlaylistEntry(index) => {
            Some(PlaylistChange::Select(index))
        }
        super::session::PlaybackOutcome::Completed if autoplay_next && controls.next_available => {
            Some(PlaylistChange::Step(
                super::super::playlist::PlaylistStep::Next,
            ))
        }
        _ => None,
    }
}

fn force_media_title_for_entry<'a>(
    current: &Path,
    initial: &Path,
    forced: Option<&'a str>,
) -> Option<&'a str> {
    (current == initial).then_some(forced).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::playback::session::PlaybackOutcome;
    use crate::app::playlist::PlaylistStep;

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
}
