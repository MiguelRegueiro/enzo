//! Options menu behavior; persistence belongs to config and drawing to overlay.

use std::path::PathBuf;

use crate::{
    cli::OptionsInput,
    config::{
        DEFAULT_ACCENT_COLOR, MAX_BACKGROUND_BLUR, parse_hex_color, save_accent_color,
        save_background_blur, save_panel_opacity, save_playback_controls_autohide,
    },
    overlay::{HexInputState, OptionsAction, OptionsMenuState, OptionsSetting, SecondsInputState},
};

use super::session::PlaybackOutcome;

const ACCENTS: &[(&str, [u8; 3])] = &[
    ("Default", DEFAULT_ACCENT_COLOR),
    ("Orange", [0xfb, 0x92, 0x3c]),
    ("Amber", [0xfb, 0xbf, 0x24]),
    ("Green", [0x22, 0xc5, 0x5e]),
    ("Blue", [0x60, 0xa5, 0xfa]),
    ("Violet", [0xa7, 0x8b, 0xfa]),
];
const CUSTOM: usize = ACCENTS.len();

pub(super) struct OptionsMenu {
    pub(super) state: Option<OptionsMenuState>,
    pub(super) color: [u8; 3],
    pub(super) custom_color: Option<[u8; 3]>,
    pub(super) panel_opacity: u8,
    pub(super) background_blur: u8,
    pub(super) playback_controls_autohide: u16,
    pub(super) custom_playback_controls_autohide: Option<u16>,
    path: Option<PathBuf>,
    selected: usize,
    selected_setting: OptionsSetting,
}

impl OptionsMenu {
    #[cfg(test)]
    pub(super) fn new(color: [u8; 3], path: Option<PathBuf>) -> Self {
        Self::with_appearance(
            color,
            crate::config::DEFAULT_PANEL_OPACITY,
            crate::config::DEFAULT_BACKGROUND_BLUR,
            crate::config::DEFAULT_PLAYBACK_CONTROLS_AUTOHIDE,
            None,
            path,
        )
    }

    pub(super) fn with_appearance(
        color: [u8; 3],
        panel_opacity: u8,
        background_blur: u8,
        playback_controls_autohide: u16,
        custom_playback_controls_autohide: Option<u16>,
        path: Option<PathBuf>,
    ) -> Self {
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
            panel_opacity,
            background_blur,
            playback_controls_autohide,
            custom_playback_controls_autohide: custom_playback_controls_autohide.or_else(|| {
                is_custom_autohide(playback_controls_autohide).then_some(playback_controls_autohide)
            }),
            selected_setting: OptionsSetting::AccentColor,
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
            selected_setting: self.selected_setting,
            panel_opacity: self.panel_opacity,
            background_blur: self.background_blur,
            playback_controls_autohide: self.playback_controls_autohide,
            seconds_editor: (crate::overlay::autohide_choice(self.playback_controls_autohide) == 4)
                .then(|| SecondsInputState {
                    text: self.playback_controls_autohide.to_string(),
                    focused: self.selected_setting == OptionsSetting::PlaybackControlsAutohide,
                    selected: false,
                }),
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
        if matches!(input, OptionsInput::Character('o')) {
            match self.selected_setting {
                OptionsSetting::AccentColor => {
                    if let Some(color) = state
                        .editor
                        .as_ref()
                        .and_then(|editor| parse_hex_color(&editor.text).ok())
                    {
                        self.apply(CUSTOM, Some(color));
                    }
                }
                OptionsSetting::PlaybackControlsAutohide => {
                    if let Some(seconds) = state
                        .seconds_editor
                        .as_ref()
                        .and_then(|editor| editor.text.parse::<u16>().ok())
                        .filter(|seconds| (1..=9_999).contains(seconds))
                    {
                        self.set_playback_controls_autohide(seconds);
                    }
                }
                OptionsSetting::PanelOpacity | OptionsSetting::BackgroundBlur => {}
            }
            self.state = None;
            return None;
        }
        if matches!(input, OptionsInput::Character('q'))
            && self.selected_setting == OptionsSetting::AccentColor
        {
            if let Some(color) = state
                .editor
                .as_ref()
                .and_then(|editor| parse_hex_color(&editor.text).ok())
            {
                self.apply(CUSTOM, Some(color));
            }
            return Some(PlaybackOutcome::Quit);
        }
        if let OptionsInput::Navigate(direction) = input {
            if self.selected_setting == OptionsSetting::PlaybackControlsAutohide
                && let Some(seconds) = state
                    .seconds_editor
                    .as_ref()
                    .and_then(|editor| editor.text.parse::<u16>().ok())
                    .filter(|seconds| (1..=9_999).contains(seconds))
            {
                self.set_playback_controls_autohide(seconds);
            }
            self.selected_setting = self.selected_setting.navigate(direction);
            self.open();
            return None;
        }
        if matches!(input, OptionsInput::Confirm)
            && self.selected_setting == OptionsSetting::AccentColor
        {
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
        if self.selected_setting == OptionsSetting::PlaybackControlsAutohide {
            match input {
                OptionsInput::Character('q') => {
                    if let Some(seconds) = state
                        .seconds_editor
                        .as_ref()
                        .and_then(|editor| editor.text.parse::<u16>().ok())
                        .filter(|seconds| (1..=9_999).contains(seconds))
                    {
                        self.set_playback_controls_autohide(seconds);
                    }
                    return Some(PlaybackOutcome::Quit);
                }
                OptionsInput::Character('Q') => return Some(PlaybackOutcome::QuitWithoutSaving),
                _ => {}
            }
            if matches!(input, OptionsInput::Confirm) {
                let Some(editor) = state.seconds_editor.as_ref() else {
                    self.state = None;
                    return None;
                };
                match editor.text.parse::<u16>() {
                    Ok(seconds @ 1..=9_999) => {
                        if self.set_playback_controls_autohide(seconds) {
                            self.state = None;
                        }
                    }
                    _ => state.error = Some("Use whole seconds from 1 to 9999".into()),
                }
                return None;
            }
            if let Some(editor) = state.seconds_editor.as_mut() {
                match input {
                    OptionsInput::Character(ch) if ch.is_ascii_digit() => {
                        if editor.selected {
                            editor.text.clear();
                            editor.selected = false;
                        }
                        if editor.text.len() < 4 {
                            editor.text.push(ch);
                        }
                        state.error = None;
                    }
                    OptionsInput::Paste(text) => {
                        let text = text.trim();
                        if text.chars().all(|ch| ch.is_ascii_digit()) {
                            editor.text = text.chars().take(4).collect();
                            editor.selected = false;
                            state.error = None;
                        }
                    }
                    OptionsInput::Backspace | OptionsInput::Delete => {
                        if editor.selected {
                            editor.text.clear();
                            editor.selected = false;
                        } else {
                            editor.text.pop();
                        }
                        state.error = None;
                    }
                    OptionsInput::SelectAll => editor.selected = true,
                    OptionsInput::Cycle(direction) => self.cycle_autohide(direction),
                    _ => {}
                }
                return None;
            }
        }
        if self.selected_setting == OptionsSetting::AccentColor
            && let Some(editor) = state.editor.as_mut()
        {
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
            OptionsInput::Character('q') => return Some(PlaybackOutcome::Quit),
            OptionsInput::Character('Q') => return Some(PlaybackOutcome::QuitWithoutSaving),
            OptionsInput::Cycle(direction)
                if self.selected_setting == OptionsSetting::AccentColor =>
            {
                self.cycle(direction)
            }
            OptionsInput::Cycle(direction) => self.adjust_selected_setting(direction),
            OptionsInput::Confirm => self.state = None,
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
            OptionsAction::Cycle(direction)
                if self.selected_setting == OptionsSetting::AccentColor =>
            {
                self.cycle(direction)
            }
            OptionsAction::Cycle(direction) => self.adjust_selected_setting(direction),
            OptionsAction::CycleAccent(direction) => {
                self.selected_setting = OptionsSetting::AccentColor;
                self.cycle(direction);
            }
            OptionsAction::AdjustOpacity(direction) => self.adjust_opacity(direction),
            OptionsAction::SetOpacity(opacity) => self.set_opacity(opacity),
            OptionsAction::AdjustBlur(direction) => self.adjust_blur(direction),
            OptionsAction::SetBlur(blur) => self.set_blur(blur),
            OptionsAction::CycleAutohide(direction) => self.cycle_autohide(direction),
            OptionsAction::SecondsCursor => {
                self.selected_setting = OptionsSetting::PlaybackControlsAutohide;
                if let Some(editor) = self
                    .state
                    .as_mut()
                    .and_then(|state| state.seconds_editor.as_mut())
                {
                    editor.focused = true;
                    editor.selected = true;
                }
                if let Some(state) = self.state.as_mut() {
                    state.selected_setting = OptionsSetting::PlaybackControlsAutohide;
                }
            }
            OptionsAction::Cursor(cursor) => {
                self.selected_setting = OptionsSetting::AccentColor;
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

    fn adjust_opacity(&mut self, direction: i32) {
        let opacity = (i32::from(self.panel_opacity) + direction * 5).clamp(0, 100) as u8;
        self.set_opacity(opacity);
    }

    fn adjust_selected_setting(&mut self, direction: i32) {
        match self.selected_setting {
            OptionsSetting::AccentColor => self.cycle(direction),
            OptionsSetting::PanelOpacity => self.adjust_opacity(direction),
            OptionsSetting::BackgroundBlur => self.adjust_blur(direction),
            OptionsSetting::PlaybackControlsAutohide => self.cycle_autohide(direction),
        }
    }

    fn set_opacity(&mut self, opacity: u8) {
        let result = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("config directory is unavailable"))
            .and_then(|path| save_panel_opacity(path, opacity));
        if let Err(error) = result {
            if let Some(state) = self.state.as_mut() {
                state.error = Some(format!("Not saved: {error:#}"));
            }
            return;
        }
        self.panel_opacity = opacity;
        self.selected_setting = OptionsSetting::PanelOpacity;
        self.open();
    }

    fn adjust_blur(&mut self, direction: i32) {
        let blur = (i32::from(self.background_blur) + direction * 2)
            .clamp(0, i32::from(MAX_BACKGROUND_BLUR)) as u8;
        self.set_blur(blur);
    }

    fn set_blur(&mut self, blur: u8) {
        let blur = blur.min(MAX_BACKGROUND_BLUR) & !1;
        let result = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("config directory is unavailable"))
            .and_then(|path| save_background_blur(path, blur));
        if let Err(error) = result {
            if let Some(state) = self.state.as_mut() {
                state.error = Some(format!("Not saved: {error:#}"));
            }
            return;
        }
        self.background_blur = blur;
        self.selected_setting = OptionsSetting::BackgroundBlur;
        self.open();
    }

    fn cycle_autohide(&mut self, direction: i32) {
        let choice = crate::overlay::autohide_choice(self.playback_controls_autohide);
        self.set_autohide_choice((choice as i32 + direction).rem_euclid(5) as usize);
    }

    fn set_autohide_choice(&mut self, choice: usize) {
        self.selected_setting = OptionsSetting::PlaybackControlsAutohide;
        match choice.min(4) {
            0 => {
                self.set_playback_controls_autohide(2);
            }
            1 => {
                self.set_playback_controls_autohide(4);
            }
            2 => {
                self.set_playback_controls_autohide(8);
            }
            3 => {
                self.set_playback_controls_autohide(0);
            }
            _ => {
                if let Some(state) = self.state.as_mut() {
                    state.seconds_editor = Some(SecondsInputState {
                        text: self
                            .custom_playback_controls_autohide
                            .map_or_else(String::new, |seconds| seconds.to_string()),
                        focused: true,
                        selected: true,
                    });
                    state.error = None;
                }
            }
        }
    }

    fn set_playback_controls_autohide(&mut self, seconds: u16) -> bool {
        let result = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("config directory is unavailable"))
            .and_then(|path| {
                save_playback_controls_autohide(
                    path,
                    seconds,
                    is_custom_autohide(seconds)
                        .then_some(seconds)
                        .or(self.custom_playback_controls_autohide),
                )
            });
        if let Err(error) = result {
            if let Some(state) = self.state.as_mut() {
                state.error = Some(format!("Not saved: {error:#}"));
            }
            return false;
        }
        self.playback_controls_autohide = seconds;
        if is_custom_autohide(seconds) {
            self.custom_playback_controls_autohide = Some(seconds);
        }
        self.open();
        true
    }
}

fn is_custom_autohide(seconds: u16) -> bool {
    seconds != 0 && !matches!(seconds, 2 | 4 | 8)
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
