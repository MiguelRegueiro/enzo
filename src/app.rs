mod launcher;
mod playback;
mod playlist;
mod terminal_input;

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

    if let Some(path) = options.path {
        let _terminal = TerminalGuard::enter()?;
        playback::play(
            path,
            options.sub_file.as_deref(),
            options.resume_enabled,
            &font_system,
        )
    } else {
        launcher::run(
            options.sub_file.as_deref(),
            options.resume_enabled,
            &font_system,
        )
    }
}
