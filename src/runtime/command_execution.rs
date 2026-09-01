use std::sync::Arc;

use anyhow::{Context, Result};

use crate::{
    cli::{Options, run_media_drop_launcher},
    font::FontSystem,
    playback,
    resume::ResumeTracker,
    terminal::TerminalGuard,
};

use super::{
    shutdown_signal::install_shutdown_handlers,
    terminal_preflight::{prepare_tmux_passthrough, require_supported_terminal},
};

pub(super) fn execute(options: Options) -> Result<()> {
    if options.clear_resume {
        let removed = ResumeTracker::clear_all().context("failed to clear saved playback state")?;
        println!("Cleared {removed} saved playback state file(s).");
        return Ok(());
    }
    install_shutdown_handlers().context("failed to install shutdown handlers")?;
    let font_system = FontSystem::discover();
    require_supported_terminal(options.force)?;
    prepare_tmux_passthrough();

    let playback_options = playback::PlaybackOptions {
        resume_enabled: options.resume_enabled,
        autoplay_next: options.autoplay_next,
        volume_max: options.volume_max,
        accent_color: options.accent_color,
        force_media_title: options
            .force_media_title
            .filter(|title| !title.is_empty())
            .map(Arc::from),
    };

    if let Some(path) = options.path {
        let _terminal = TerminalGuard::enter()?;
        playback::play(
            path,
            options.sub_file.as_deref(),
            playback_options,
            &font_system,
        )
    } else {
        run_media_drop_launcher(options.sub_file.as_deref(), playback_options, &font_system)
    }
}
