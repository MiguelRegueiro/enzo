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

// Clear every high-volume/extended mouse mode before enabling the bounded
// interaction modes Enzo actually needs. This also repairs terminal state left
// behind by an interrupted older Enzo process.
const ENABLE_MOUSE_TRACKING: &[u8] =
    b"\x1b[?1003l\x1b[?1015l\x1b[?1016l\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const DISABLE_MOUSE_TRACKING: &[u8] =
    b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1015l\x1b[?1016l";

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

#[cfg(test)]
mod tests {
    use super::{DISABLE_MOUSE_TRACKING, ENABLE_MOUSE_TRACKING};

    #[test]
    fn pointer_tracking_keeps_clicks_and_drags_but_disables_flood_modes() {
        for mode in [b"\x1b[?1000h", b"\x1b[?1002h", b"\x1b[?1006h"] {
            assert!(
                ENABLE_MOUSE_TRACKING
                    .windows(mode.len())
                    .any(|enabled| enabled == mode)
            );
        }
        for mode in [b"\x1b[?1003l", b"\x1b[?1015l", b"\x1b[?1016l"] {
            assert!(
                ENABLE_MOUSE_TRACKING
                    .windows(mode.len())
                    .any(|disabled| disabled == mode)
            );
        }
        for mode in [b"\x1b[?1003h", b"\x1b[?1015h", b"\x1b[?1016h"] {
            assert!(
                !ENABLE_MOUSE_TRACKING
                    .windows(mode.len())
                    .any(|enabled| enabled == mode)
            );
        }
    }

    #[test]
    fn teardown_disables_every_mouse_mode_enzo_may_encounter() {
        for mode in [
            b"\x1b[?1000l",
            b"\x1b[?1002l",
            b"\x1b[?1003l",
            b"\x1b[?1006l",
            b"\x1b[?1015l",
            b"\x1b[?1016l",
        ] {
            assert!(
                DISABLE_MOUSE_TRACKING
                    .windows(mode.len())
                    .any(|disabled| disabled == mode)
            );
        }
    }
}
