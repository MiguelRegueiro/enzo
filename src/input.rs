use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackCommand {
    None,
    Quit,
    TogglePause,
    ToggleMute,
    ToggleSubtitles,
    SeekBy(i32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackInput {
    pub(crate) command: PlaybackCommand,
    pub(crate) mouse_activity: bool,
    pub(crate) mouse_events: Vec<PlaybackMouse>,
    pub(crate) text: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackMouse {
    Down { column: u16, row: u16 },
    Drag { column: u16, row: u16 },
    Up { column: u16, row: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DropInput {
    pub(crate) command: DropCommand,
    pub(crate) text: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropCommand {
    None,
    Quit,
}

pub(crate) fn read_input_events() -> Result<PlaybackInput> {
    let mut input = PlaybackInput {
        command: PlaybackCommand::None,
        mouse_activity: false,
        mouse_events: Vec::new(),
        text: None,
    };
    let mut seek_delta = 0_i32;
    while event::poll(Duration::from_millis(0)).context("failed to poll terminal input")? {
        match event::read().context("failed to read terminal input")? {
            Event::Key(key) => {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
                if matches!(key.code, KeyCode::Char('q')) {
                    input.command = PlaybackCommand::Quit;
                    return Ok(input);
                }
                if matches!(key.code, KeyCode::Char(' ')) {
                    input.command = PlaybackCommand::TogglePause;
                }
                if matches!(key.code, KeyCode::Char('m')) {
                    input.command = PlaybackCommand::ToggleMute;
                }
                if matches!(key.code, KeyCode::Char('v')) {
                    input.command = PlaybackCommand::ToggleSubtitles;
                }
                if matches!(key.code, KeyCode::Right) {
                    seek_delta = seek_delta.saturating_add(1);
                }
                if matches!(key.code, KeyCode::Left) {
                    seek_delta = seek_delta.saturating_sub(1);
                }
            }
            Event::Mouse(mouse) => {
                input.mouse_activity = true;
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        input.mouse_events.push(PlaybackMouse::Down {
                            column: mouse.column,
                            row: mouse.row,
                        });
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        input.mouse_events.push(PlaybackMouse::Drag {
                            column: mouse.column,
                            row: mouse.row,
                        });
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        input.mouse_events.push(PlaybackMouse::Up {
                            column: mouse.column,
                            row: mouse.row,
                        });
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        input.command = PlaybackCommand::TogglePause;
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => {
                input.text = Some(text);
            }
            _ => {}
        }
    }

    if seek_delta != 0 && input.command == PlaybackCommand::None {
        input.command = PlaybackCommand::SeekBy(seek_delta);
    }

    Ok(input)
}

pub(crate) fn read_drop_events() -> Result<DropInput> {
    let mut input = DropInput {
        command: DropCommand::None,
        text: None,
    };

    if !event::poll(Duration::from_millis(100)).context("failed to poll terminal input")? {
        return Ok(input);
    }

    loop {
        match event::read().context("failed to read terminal input")? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    // Ignore key releases/repeats in the launcher.
                } else if matches!(key.code, KeyCode::Char('q')) {
                    input.command = DropCommand::Quit;
                    return Ok(input);
                }
            }
            Event::Paste(text) => {
                input.text = Some(text);
            }
            _ => {}
        }

        if !event::poll(Duration::from_millis(0)).context("failed to poll terminal input")? {
            return Ok(input);
        }
    }
}
