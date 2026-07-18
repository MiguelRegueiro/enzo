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
    ShowMediaInfo,
    ToggleMediaInfo,
    SeekBySeconds(i32),
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

fn seek_seconds_for_key(key: &KeyCode) -> Option<i32> {
    match key {
        KeyCode::Left => Some(-5),
        KeyCode::Right => Some(5),
        KeyCode::Down => Some(-60),
        KeyCode::Up => Some(60),
        _ => None,
    }
}

pub(crate) fn read_input_events() -> Result<PlaybackInput> {
    let mut input = PlaybackInput {
        command: PlaybackCommand::None,
        mouse_activity: false,
        mouse_events: Vec::new(),
        text: None,
    };
    let mut seek_seconds = 0_i32;
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
                if matches!(key.code, KeyCode::Char('i')) {
                    input.command = PlaybackCommand::ShowMediaInfo;
                }
                if matches!(key.code, KeyCode::Char('I')) {
                    input.command = PlaybackCommand::ToggleMediaInfo;
                }
                if let Some(seconds) = seek_seconds_for_key(&key.code) {
                    seek_seconds = seek_seconds.saturating_add(seconds);
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

    if seek_seconds != 0 && input.command == PlaybackCommand::None {
        input.command = PlaybackCommand::SeekBySeconds(seek_seconds);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_keys_map_to_fixed_seek_durations() {
        assert_eq!(seek_seconds_for_key(&KeyCode::Left), Some(-5));
        assert_eq!(seek_seconds_for_key(&KeyCode::Right), Some(5));
        assert_eq!(seek_seconds_for_key(&KeyCode::Down), Some(-60));
        assert_eq!(seek_seconds_for_key(&KeyCode::Up), Some(60));
    }

    #[test]
    fn non_seek_keys_have_no_seek_duration() {
        assert_eq!(seek_seconds_for_key(&KeyCode::Char('q')), None);
        assert_eq!(seek_seconds_for_key(&KeyCode::Enter), None);
    }
}
