//! Options menu behavior; persistence belongs to config and drawing to overlay.

use std::path::PathBuf;

use crate::{
    cli::OptionsInput,
    config::{DEFAULT_ACCENT_COLOR, parse_hex_color, save_accent_color},
    overlay::{HexInputState, OptionsAction, OptionsMenuState},
};

use super::session::PlaybackOutcome;

const ACCENTS: &[(&str, [u8; 3])] = &[
    ("Default", DEFAULT_ACCENT_COLOR),
    ("Blue", [0x44, 0x50, 0xef]),
    ("Green", [0x22, 0xc5, 0x5e]),
    ("Violet", [0xa7, 0x8b, 0xfa]),
    ("Amber", [0xfb, 0xbf, 0x24]),
    ("Cyan", [0x22, 0xd3, 0xee]),
];
const CUSTOM: usize = ACCENTS.len();

pub(super) struct OptionsMenu {
    pub(super) state: Option<OptionsMenuState>,
    pub(super) color: [u8; 3],
    pub(super) custom_color: Option<[u8; 3]>,
    path: Option<PathBuf>,
    selected: usize,
}

impl OptionsMenu {
    pub(super) fn new(color: [u8; 3], path: Option<PathBuf>) -> Self {
        let selected = ACCENTS
            .iter()
            .position(|(_, preset)| *preset == color)
            .unwrap_or(CUSTOM);
        Self {
            state: None,
            color,
            path,
            selected,
            custom_color: (selected == CUSTOM).then_some(color),
        }
    }

    pub(super) fn open(&mut self) {
        let text = self.custom_color.map_or_else(
            || "#".to_owned(),
            |[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"),
        );
        self.state = Some(OptionsMenuState {
            position: self.selected + 1,
            count: ACCENTS.len() + 1,
            name: ACCENTS
                .get(self.selected)
                .map_or("Custom", |(name, _)| name),
            color: self.color,
            editor: (self.selected == CUSTOM).then(|| HexInputState {
                cursor: text.len(),
                text,
                focused: self.custom_color.is_none(),
                selected: false,
            }),
            error: None,
        });
    }

    /// Incomplete drafts and closed menus use the last successfully saved color.
    pub(super) fn preview_color(&self) -> [u8; 3] {
        self.state
            .as_ref()
            .and_then(|state| state.editor.as_ref())
            .and_then(|editor| parse_hex_color(&editor.text).ok())
            .unwrap_or(self.color)
    }

    pub(super) fn overlay_state(&self) -> Option<OptionsMenuState> {
        self.state.clone().map(|mut state| {
            state.color = self.preview_color();
            state
        })
    }

    pub(super) fn input(&mut self, input: OptionsInput) -> Option<PlaybackOutcome> {
        let state = self.state.as_mut()?;
        if matches!(input, OptionsInput::Close) {
            self.state = None;
            return None;
        }
        if matches!(input, OptionsInput::Confirm) {
            if let Some(editor) = state.editor.as_ref().filter(|editor| editor.text.len() > 1) {
                match parse_hex_color(&editor.text) {
                    Ok(color) => {
                        if self.apply(CUSTOM, Some(color)) {
                            self.state = None;
                        }
                    }
                    Err(_) => state.error = Some("Use #RRGGBB (six hex digits)".into()),
                }
            } else {
                self.state = None;
            }
            return None;
        }
        if let Some(editor) = state.editor.as_mut() {
            if matches!(input, OptionsInput::Focus) {
                editor.focused = !editor.focused;
                return None;
            }
            if matches!(&input, OptionsInput::Character(ch) if ch.is_ascii_hexdigit() || *ch == '#')
                || matches!(
                    input,
                    OptionsInput::Paste(_)
                        | OptionsInput::Backspace
                        | OptionsInput::Delete
                        | OptionsInput::Home
                        | OptionsInput::End
                        | OptionsInput::SelectAll
                        | OptionsInput::DeleteToStart
                        | OptionsInput::DeleteToEnd
                )
            {
                editor.focused = true;
            }
            if editor.focused {
                match input {
                    OptionsInput::Cycle(direction) if editor.text.len() == 1 => {
                        self.cycle(direction);
                    }
                    OptionsInput::Paste(text) => {
                        let text = text.trim();
                        let hex = format!("#{}", text.strip_prefix('#').unwrap_or(text));
                        if parse_hex_color(&hex).is_ok() {
                            editor.text = hex;
                            editor.cursor = editor.text.len();
                            editor.selected = false;
                            state.error = None;
                        } else {
                            state.error = Some("Use #RRGGBB (six hex digits)".into());
                        }
                    }
                    input => {
                        edit_hex(editor, input);
                        state.error = None;
                    }
                }
                return None;
            }
        }
        match input {
            OptionsInput::Character('o') => self.state = None,
            OptionsInput::Character('q') => return Some(PlaybackOutcome::Quit),
            OptionsInput::Character('Q') => return Some(PlaybackOutcome::QuitWithoutSaving),
            OptionsInput::Cycle(direction) => self.cycle(direction),
            _ => {}
        }
        None
    }

    pub(super) fn action(&mut self, action: OptionsAction) {
        if self.state.is_none() {
            return;
        }
        match action {
            OptionsAction::Close => self.state = None,
            OptionsAction::Cycle(direction) => self.cycle(direction),
            OptionsAction::Cursor(cursor) => {
                if let Some(editor) = self.state.as_mut().and_then(|state| state.editor.as_mut()) {
                    editor.focused = true;
                    editor.selected = false;
                    editor.cursor = cursor.clamp(1, editor.text.len());
                }
            }
        }
    }

    fn cycle(&mut self, direction: i32) {
        let index = (self.selected as i32 + direction).rem_euclid((CUSTOM + 1) as i32) as usize;
        if index == CUSTOM {
            // Selecting Custom exposes the field immediately. It only saves on Enter.
            self.selected = CUSTOM;
            self.open();
        } else {
            self.apply(index, (index != 0).then_some(ACCENTS[index].1));
        }
    }

    fn apply(&mut self, index: usize, color: Option<[u8; 3]>) -> bool {
        let custom_color = if index == CUSTOM {
            color
        } else {
            self.custom_color
        };
        let result = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("config directory is unavailable"))
            .and_then(|path| save_accent_color(path, color, custom_color));
        if let Err(error) = result {
            if let Some(state) = self.state.as_mut() {
                state.error = Some(format!("Not saved: {error:#}"));
            }
            return false;
        }
        self.color = color.unwrap_or(DEFAULT_ACCENT_COLOR);
        self.selected = index;
        if index == CUSTOM {
            self.custom_color = Some(self.color);
            if let Some(state) = self.state.as_mut() {
                state.color = self.color;
                state.error = None;
            }
        } else {
            self.open();
        }
        true
    }
}

fn edit_hex(editor: &mut HexInputState, input: OptionsInput) {
    match input {
        OptionsInput::Cycle(direction) => {
            editor.cursor = if editor.selected {
                if direction < 0 { 1 } else { editor.text.len() }
            } else {
                (editor.cursor as i32 + direction).clamp(1, editor.text.len() as i32) as usize
            };
            editor.selected = false;
        }
        OptionsInput::Home | OptionsInput::End => {
            editor.cursor = if input == OptionsInput::Home {
                1
            } else {
                editor.text.len()
            };
            editor.selected = false;
        }
        OptionsInput::SelectAll => editor.selected = true,
        OptionsInput::DeleteToStart | OptionsInput::DeleteToEnd => {
            if editor.selected {
                editor.text.truncate(1);
                editor.cursor = 1;
                editor.selected = false;
            } else if input == OptionsInput::DeleteToStart {
                editor.text.drain(1..editor.cursor);
                editor.cursor = 1;
            } else {
                editor.text.truncate(editor.cursor);
            }
        }
        OptionsInput::Character(ch) if ch.is_ascii_hexdigit() || ch == '#' => {
            if editor.selected {
                editor.text.truncate(1);
                editor.cursor = 1;
                editor.selected = false;
            }
            if ch != '#' && editor.text.len() < 7 {
                editor.text.insert(editor.cursor, ch);
                editor.cursor += 1;
            }
        }
        OptionsInput::Backspace | OptionsInput::Delete => {
            if editor.selected {
                editor.text.truncate(1);
                editor.cursor = 1;
                editor.selected = false;
            } else if input == OptionsInput::Backspace && editor.cursor > 1 {
                editor.cursor -= 1;
                editor.text.remove(editor.cursor);
            } else if input == OptionsInput::Delete && editor.cursor < editor.text.len() {
                editor.text.remove(editor.cursor);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "tests/options.rs"]
mod tests;
