use std::io::{self, Write};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

use super::kitty_graphics::clear_all_kitty_images;

const ENABLE_MOUSE_TRACKING: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";
const DISABLE_MOUSE_TRACKING: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1016l";

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
            EnableBracketedPaste
        )
        .context("failed to enter terminal playback mode")?;
        stdout
            .write_all(ENABLE_MOUSE_TRACKING)
            .context("failed to enable terminal mouse tracking")?;
        stdout.flush().context("failed to flush terminal setup")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = clear_all_kitty_images(&mut stdout);
        let _ = stdout.write_all(DISABLE_MOUSE_TRACKING);
        let _ = execute!(stdout, DisableBracketedPaste, Show, LeaveAlternateScreen);
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}
