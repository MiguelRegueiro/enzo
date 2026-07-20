use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};

const INPUT_EVENTS_PER_TICK: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackCommand {
    None,
    Quit,
    TogglePause,
    ToggleMute,
    ToggleSubtitles,
    ToggleAudioPicker,
    ToggleSubtitlePicker,
    ShowMediaInfo,
    ToggleMediaInfo,
    ConfirmPicker,
    SeekBySeconds { seconds: i32, picker_direction: i32 },
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
    Move { column: u16, row: u16 },
    ScrollUp,
    ScrollDown,
}

impl PlaybackMouse {
    pub(crate) fn interrupts_keyboard_seek(self) -> bool {
        !matches!(self, PlaybackMouse::Move { .. })
    }
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

fn picker_direction_for_key(key: &KeyCode) -> Option<i32> {
    match key {
        KeyCode::Up => Some(-1),
        KeyCode::Down => Some(1),
        _ => None,
    }
}

fn playback_command_for_key(key: &KeyCode) -> PlaybackCommand {
    match key {
        KeyCode::Char('q') => PlaybackCommand::Quit,
        KeyCode::Char(' ') => PlaybackCommand::TogglePause,
        KeyCode::Char('m') => PlaybackCommand::ToggleMute,
        KeyCode::Char('v') => PlaybackCommand::ToggleSubtitles,
        KeyCode::Char('a') => PlaybackCommand::ToggleAudioPicker,
        KeyCode::Char('s') => PlaybackCommand::ToggleSubtitlePicker,
        KeyCode::Char('i') => PlaybackCommand::ShowMediaInfo,
        KeyCode::Char('I') => PlaybackCommand::ToggleMediaInfo,
        KeyCode::Enter => PlaybackCommand::ConfirmPicker,
        _ => PlaybackCommand::None,
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
    let mut picker_direction = 0_i32;
    // Passive movement can keep the terminal queue continuously non-empty.
    // Bound each drain so input can never starve frame delivery or UI expiry.
    for _ in 0..INPUT_EVENTS_PER_TICK {
        if !event::poll(Duration::from_millis(0)).context("failed to poll terminal input")? {
            break;
        }
        match event::read().context("failed to read terminal input")? {
            Event::Key(key) => {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
                let command = playback_command_for_key(&key.code);
                if command == PlaybackCommand::Quit {
                    input.command = command;
                    return Ok(input);
                }
                if command != PlaybackCommand::None {
                    input.command = command;
                }
                if let Some(seconds) = seek_seconds_for_key(&key.code) {
                    seek_seconds = seek_seconds.saturating_add(seconds);
                }
                if let Some(direction) = picker_direction_for_key(&key.code) {
                    picker_direction = picker_direction.saturating_add(direction);
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
                    MouseEventKind::ScrollUp => {
                        input.mouse_events.push(PlaybackMouse::ScrollUp);
                    }
                    MouseEventKind::ScrollDown => {
                        input.mouse_events.push(PlaybackMouse::ScrollDown);
                    }
                    MouseEventKind::Moved => {
                        input.mouse_events.push(PlaybackMouse::Move {
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
        input.command = PlaybackCommand::SeekBySeconds {
            seconds: seek_seconds,
            picker_direction,
        };
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

    #[test]
    fn only_vertical_arrows_drive_picker_navigation() {
        assert_eq!(picker_direction_for_key(&KeyCode::Up), Some(-1));
        assert_eq!(picker_direction_for_key(&KeyCode::Down), Some(1));
        assert_eq!(picker_direction_for_key(&KeyCode::Left), None);
        assert_eq!(picker_direction_for_key(&KeyCode::Right), None);
    }

    #[test]
    fn passive_mouse_movement_does_not_interrupt_keyboard_seek() {
        assert!(!PlaybackMouse::Move { column: 1, row: 1 }.interrupts_keyboard_seek());
        assert!(PlaybackMouse::Down { column: 1, row: 1 }.interrupts_keyboard_seek());
        assert!(PlaybackMouse::Drag { column: 1, row: 1 }.interrupts_keyboard_seek());
        assert!(PlaybackMouse::Up { column: 1, row: 1 }.interrupts_keyboard_seek());
        assert!(PlaybackMouse::ScrollUp.interrupts_keyboard_seek());
        assert!(PlaybackMouse::ScrollDown.interrupts_keyboard_seek());
    }

    #[test]
    fn letter_keys_map_to_playback_commands() {
        assert_eq!(
            playback_command_for_key(&KeyCode::Char('a')),
            PlaybackCommand::ToggleAudioPicker
        );
        assert_eq!(
            playback_command_for_key(&KeyCode::Char('s')),
            PlaybackCommand::ToggleSubtitlePicker
        );
        assert_eq!(
            playback_command_for_key(&KeyCode::Char('v')),
            PlaybackCommand::ToggleSubtitles
        );
        assert_eq!(
            playback_command_for_key(&KeyCode::Enter),
            PlaybackCommand::ConfirmPicker
        );
    }
}
