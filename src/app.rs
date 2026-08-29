mod launcher;
mod playback;
mod playlist;
mod terminal_input;

use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::{
    cli::Options,
    font_system::FontSystem,
    resume::ResumeTracker,
    shutdown,
    terminal::{TerminalGuard, enable_tmux_passthrough, inside_tmux, looks_like_kitty},
};

pub(crate) fn run(options: Options) -> Result<()> {
    if options.clear_resume {
        let removed = ResumeTracker::clear_all().context("failed to clear saved playback state")?;
        println!("Cleared {removed} saved playback state file(s).");
        return Ok(());
    }
    shutdown::install_signal_handlers().context("failed to install shutdown handlers")?;
    let font_system = FontSystem::discover();
    if !options.force && !looks_like_kitty() {
        bail!("Enzo requires Kitty; pass --force to bypass terminal detection");
    }

    if inside_tmux() {
        enable_tmux_passthrough();
    }

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
        launcher::run(options.sub_file.as_deref(), playback_options, &font_system)
    }
}
