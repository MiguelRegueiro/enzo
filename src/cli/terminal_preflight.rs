use anyhow::{Result, bail};

use crate::terminal::{enable_tmux_passthrough, inside_tmux, looks_like_kitty};

pub(super) fn require_supported_terminal(force: bool) -> Result<()> {
    if !force && !looks_like_kitty() {
        bail!("Enzo requires Kitty; pass --force to bypass terminal detection");
    }
    Ok(())
}

pub(super) fn prepare_tmux_passthrough() {
    if inside_tmux() {
        enable_tmux_passthrough();
    }
}

#[cfg(test)]
#[path = "tests/terminal_preflight.rs"]
mod tests;
