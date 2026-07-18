mod cli;
mod launcher;
mod path_input;
mod playback;
mod terminal_input;

use std::env;

use anyhow::{Context, Result, bail};

use crate::{
    font_system::FontSystem,
    resume::ResumeTracker,
    shutdown,
    terminal::{TerminalGuard, enable_tmux_passthrough, inside_tmux, looks_like_kitty},
};

use cli::parse_args;

pub(crate) fn run() -> Result<()> {
    let config = parse_args(env::args_os().skip(1))?;
    if config.clear_resume {
        let removed = ResumeTracker::clear_all().context("failed to clear saved playback state")?;
        println!("Cleared {removed} saved playback state file(s).");
        return Ok(());
    }
    shutdown::install_signal_handlers().context("failed to install shutdown handlers")?;
    let font_system = FontSystem::discover();
    if !config.force && !looks_like_kitty() {
        bail!(
            "Enzo targets Kitty graphics; run from kitty or pass --force if your terminal is compatible"
        );
    }

    if inside_tmux() {
        enable_tmux_passthrough();
    }

    if let Some(path) = config.path {
        let _terminal = TerminalGuard::enter()?;
        playback::play(
            path,
            config.sub_file.as_deref(),
            config.resume_enabled,
            &font_system,
        )
    } else {
        launcher::run(
            config.sub_file.as_deref(),
            config.resume_enabled,
            &font_system,
        )
    }
}
