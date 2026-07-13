use std::io::{self, Write};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

use super::kitty_graphics::clear_all_kitty_images;

pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw terminal mode")?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            Clear(ClearType::All),
            Hide,
            EnableBracketedPaste,
            EnableMouseCapture
        )
        .context("failed to enter terminal playback mode")?;
        stdout
            .write_all(b"\x1b[?1016h")
            .context("failed to enable pixel mouse mode")?;
        stdout.flush().context("failed to flush terminal setup")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = clear_all_kitty_images(&mut stdout);
        let _ = stdout.write_all(b"\x1b[?1016l");
        let _ = execute!(
            stdout,
            DisableMouseCapture,
            DisableBracketedPaste,
            Show,
            LeaveAlternateScreen
        );
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}
