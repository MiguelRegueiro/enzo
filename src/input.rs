use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackInput {
    None,
    Quit,
    TogglePause,
}

pub(crate) fn read_input_events() -> Result<PlaybackInput> {
    let mut command = PlaybackInput::None;
    while event::poll(Duration::from_millis(0)).context("failed to poll terminal input")? {
        match event::read().context("failed to read terminal input")? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (matches!(key.code, KeyCode::Char('c'))
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    return Ok(PlaybackInput::Quit);
                }
                if matches!(key.code, KeyCode::Char(' ')) {
                    command = PlaybackInput::TogglePause;
                }
            }
            Event::Mouse(mouse) => {
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
                    command = PlaybackInput::TogglePause;
                }
            }
            _ => {}
        }
    }

    Ok(command)
}
