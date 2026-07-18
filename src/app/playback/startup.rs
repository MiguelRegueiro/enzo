use std::{
    io::{self, BufWriter},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};

use crate::{
    font_system::FontSystem,
    media::{AudioPlayer, probe_video},
    overlay::MediaInfo,
    resume::ResumeTracker,
    subtitle::SubtitleTrack,
};

use super::{
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
    ui::{PlaybackUi, media_title},
    view::PlaybackView,
};

pub(crate) fn play(
    path: PathBuf,
    sub_file: Option<&Path>,
    resume_enabled: bool,
    font_system: &FontSystem,
) -> Result<()> {
    let source = probe_video(&path)
        .with_context(|| format!("failed to inspect video metadata for {}", path.display()))?;
    let mut resume = ResumeTracker::open(
        &path,
        source.duration,
        resume_available(resume_enabled, source.seekable),
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
    let ui = PlaybackUi::new(media_title(&path), media_info, status_message);
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
