use super::*;
use crate::config::Config;

fn test_dir(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("enzo-options-{label}-{unique}"))
}

#[test]
fn preset_changes_are_saved_immediately_and_close_keeps_them() {
    let root = test_dir("presets");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.color, ACCENTS[1].1);
    assert_eq!(
        crate::config::Config::load(Some(&path))
            .unwrap()
            .accent_color,
        menu.color
    );
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.is_none());
    assert_eq!(menu.color, ACCENTS[1].1);
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.color, DEFAULT_ACCENT_COLOR);
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("accent_color")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn panel_opacity_is_adjusted_and_saved_from_its_selected_row() {
    let root = test_dir("opacity");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(1));
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.panel_opacity, 75);
    assert_eq!(
        menu.overlay_state().unwrap().selected_setting,
        OptionsSetting::PanelOpacity
    );
    assert_eq!(
        crate::config::Config::load(Some(&path))
            .unwrap()
            .panel_opacity,
        75
    );
    menu.input(OptionsInput::Navigate(-1));
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.color, ACCENTS[1].1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn background_blur_is_adjusted_and_saved_from_its_selected_row() {
    let root = test_dir("blur");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(-2));
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.background_blur, 14);
    assert_eq!(
        menu.overlay_state().unwrap().selected_setting,
        OptionsSetting::BackgroundBlur
    );
    assert_eq!(
        crate::config::Config::load(Some(&path))
            .unwrap()
            .background_blur,
        14
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn closing_with_escape_discards_custom_color_draft() {
    let mut menu = OptionsMenu::new([1, 2, 3], None);
    menu.open();
    menu.input(OptionsInput::Paste("#abcdef".into()));
    assert_eq!(
        menu.input(OptionsInput::Character('q')),
        Some(PlaybackOutcome::Quit)
    );
    assert_eq!(
        menu.state
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .map(|editor| editor.text.as_str()),
        Some("#abcdef")
    );
    menu.input(OptionsInput::Close);
    assert!(menu.state.is_none());
    assert_eq!(menu.color, [1, 2, 3]);
    menu.open();
    assert_eq!(
        menu.input(OptionsInput::Character('q')),
        Some(PlaybackOutcome::Quit)
    );
}

#[test]
fn normal_quit_and_close_save_valid_custom_color_drafts() {
    let root = test_dir("custom-color-close");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    menu.input(OptionsInput::Paste("123abc".into()));
    assert_eq!(menu.input(OptionsInput::Character('o')), None);
    assert_eq!(
        Config::load(Some(&path)).unwrap().accent_color,
        [0x12, 0x3a, 0xbc]
    );

    menu.open();
    menu.input(OptionsInput::Paste("abcdef".into()));
    assert_eq!(
        menu.input(OptionsInput::Character('q')),
        Some(PlaybackOutcome::Quit)
    );
    assert_eq!(
        Config::load(Some(&path)).unwrap().accent_color,
        [0xab, 0xcd, 0xef]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn options_toggle_closes_from_every_setting() {
    for steps in 0..4 {
        let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, None);
        menu.open();
        menu.input(OptionsInput::Navigate(steps));
        assert_eq!(menu.input(OptionsInput::Character('o')), None);
        assert!(
            menu.state.is_none(),
            "options remained open after {steps} moves"
        );
    }
}

#[test]
fn remembered_custom_survives_reopening_presets_and_cancelled_drafts() {
    let root = test_dir("remember-custom");
    let mut menu = OptionsMenu::new([0x12, 0x34, 0x56], Some(root.join("config.toml")));
    menu.open();
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#123456"
    );
    // Restoring a remembered value does not capture arrows until editing starts.
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.selected, 0);
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#123456"
    );
    menu.input(OptionsInput::Paste("abcdef".into()));
    menu.input(OptionsInput::Confirm);
    assert_eq!(menu.custom_color, Some([0xab, 0xcd, 0xef]));
    menu.open();
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#abcdef"
    );
    menu.input(OptionsInput::Backspace);
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#abcde"
    );
    menu.input(OptionsInput::Close);
    menu.open();
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#abcdef"
    );
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::Close);
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#abcdef"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remembered_custom_survives_restart_with_a_preset_active() {
    let root = test_dir("restart");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    menu.input(OptionsInput::Paste("123abc".into()));
    menu.input(OptionsInput::Confirm);
    menu.open();
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::Cycle(1));
    drop(menu);

    let config = crate::config::Config::load(Some(&path)).unwrap();
    assert_eq!(config.accent_color, ACCENTS[1].1);
    assert_eq!(config.custom_accent_color, Some([0x12, 0x3a, 0xbc]));
    let mut restarted = OptionsMenu::new(config.accent_color, Some(path));
    restarted.custom_color = config.custom_accent_color;
    restarted.open();
    restarted.input(OptionsInput::Cycle(-1));
    restarted.input(OptionsInput::Cycle(-1));
    assert_eq!(
        restarted
            .state
            .as_ref()
            .unwrap()
            .editor
            .as_ref()
            .unwrap()
            .text,
        "#123abc"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn save_failure_keeps_current_color_and_reports_error() {
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, None);
    menu.open();
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.color, DEFAULT_ACCENT_COLOR);
    assert!(
        menu.state
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .starts_with("Not saved:")
    );
}

#[test]
fn custom_hex_is_validated_and_saved_only_on_enter() {
    let root = test_dir("custom");
    let path = root.join("custom.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.state.as_ref().unwrap().name, "Custom");
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.is_none());
    assert!(!path.exists());
    menu.open();
    for ch in "01Af".chars() {
        menu.input(OptionsInput::Character(ch));
    }
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.as_ref().unwrap().error.is_some());
    assert!(!path.exists());
    for ch in "9c".chars() {
        menu.input(OptionsInput::Character(ch));
    }
    assert_eq!(menu.color, DEFAULT_ACCENT_COLOR);
    menu.input(OptionsInput::Confirm);
    assert_eq!(menu.color, [0x01, 0xaf, 0x9c]);
    assert!(menu.state.is_none());
    assert_eq!(
        crate::config::Config::load(Some(&path))
            .unwrap()
            .accent_color,
        menu.color
    );
    for text in ["ef4444", "#ef4444", "  EF4444\n"] {
        menu.open();
        menu.input(OptionsInput::Paste(text.into()));
        assert!(menu.state.as_ref().unwrap().error.is_none());
        menu.input(OptionsInput::Confirm);
        assert!(menu.state.is_none());
        assert_eq!(menu.color, [0xef, 0x44, 0x44]);
        assert_eq!(
            crate::config::Config::load(Some(&path))
                .unwrap()
                .accent_color,
            menu.color
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn custom_preview_tracks_valid_input_without_saving_drafts() {
    let root = test_dir("preview");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    for ch in "12345".chars() {
        menu.input(OptionsInput::Character(ch));
        assert_eq!(menu.preview_color(), DEFAULT_ACCENT_COLOR);
    }
    menu.input(OptionsInput::Character('6'));
    assert_eq!(menu.preview_color(), [0x12, 0x34, 0x56]);
    assert_eq!(menu.overlay_state().unwrap().color, menu.preview_color());
    assert_eq!(menu.color, DEFAULT_ACCENT_COLOR);
    assert_eq!(menu.custom_color, None);
    assert!(!path.exists());

    menu.input(OptionsInput::Backspace);
    assert_eq!(menu.preview_color(), DEFAULT_ACCENT_COLOR);
    assert_eq!(menu.overlay_state().unwrap().color, DEFAULT_ACCENT_COLOR);
    menu.input(OptionsInput::Character('a'));
    assert_eq!(menu.preview_color(), [0x12, 0x34, 0x5a]);
    menu.input(OptionsInput::SelectAll);
    menu.input(OptionsInput::Backspace);
    assert_eq!(menu.preview_color(), DEFAULT_ACCENT_COLOR);

    menu.input(OptionsInput::Paste("abcdef".into()));
    assert_eq!(menu.preview_color(), [0xab, 0xcd, 0xef]);
    assert_eq!(menu.overlay_state().unwrap().color, menu.preview_color());
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.is_none());
    assert_eq!(menu.color, [0xab, 0xcd, 0xef]);
    assert_eq!(menu.preview_color(), menu.color);
    assert_eq!(menu.custom_color, Some(menu.color));
    assert_eq!(
        crate::config::Config::load(Some(&path))
            .unwrap()
            .accent_color,
        menu.color
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn closing_or_switching_presets_discards_the_custom_preview() {
    let root = test_dir("cancel-preview");
    let mut menu = OptionsMenu::new([1, 2, 3], Some(root.join("config.toml")));
    for pointer in [false, true] {
        menu.open();
        menu.input(OptionsInput::Paste("abcdef".into()));
        assert_eq!(menu.preview_color(), [0xab, 0xcd, 0xef]);
        if pointer {
            menu.action(OptionsAction::Close);
        } else {
            menu.input(OptionsInput::Close);
        }
        assert_eq!(menu.preview_color(), [1, 2, 3]);
        assert_eq!(menu.custom_color, Some([1, 2, 3]));
    }
    menu.open();
    menu.input(OptionsInput::Paste("abcdef".into()));
    menu.action(OptionsAction::Cycle(1));
    assert_eq!(menu.preview_color(), DEFAULT_ACCENT_COLOR);
    assert_eq!(menu.overlay_state().unwrap().color, DEFAULT_ACCENT_COLOR);
    menu.action(OptionsAction::Cycle(-1));
    assert_eq!(menu.preview_color(), [1, 2, 3]);
    assert_eq!(menu.color, DEFAULT_ACCENT_COLOR);
    menu.input(OptionsInput::Close);
    menu.open();
    assert_eq!(menu.preview_color(), [1, 2, 3]);
    menu.input(OptionsInput::Backspace);
    assert_eq!(menu.preview_color(), DEFAULT_ACCENT_COLOR);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_custom_save_does_not_commit_the_preview() {
    let mut menu = OptionsMenu::new([1, 2, 3], None);
    menu.open();
    menu.input(OptionsInput::Paste("abcdef".into()));
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.as_ref().unwrap().error.is_some());
    assert_eq!(menu.preview_color(), [0xab, 0xcd, 0xef]);
    assert_eq!(menu.color, [1, 2, 3]);
    assert_eq!(menu.custom_color, Some([1, 2, 3]));
    menu.input(OptionsInput::Close);
    assert_eq!(menu.preview_color(), [1, 2, 3]);
}

#[test]
fn custom_caret_supports_navigation_insertion_and_deletion() {
    let mut menu = OptionsMenu::new([0x12, 0x34, 0x56], None);
    menu.open();
    menu.input(OptionsInput::Paste("#123456".into()));
    menu.input(OptionsInput::Home);
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::Delete);
    menu.input(OptionsInput::Character('a'));
    let editor = menu.state.as_ref().unwrap().editor.as_ref().unwrap();
    assert_eq!(editor.text, "#1a3456");
    assert_eq!(editor.cursor, 3);
    menu.input(OptionsInput::Backspace);
    menu.input(OptionsInput::End);
    menu.input(OptionsInput::Character('f'));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#13456f"
    );
    menu.input(OptionsInput::SelectAll);
    menu.input(OptionsInput::Character('b'));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#b"
    );
}

#[test]
fn word_deletion_preserves_the_other_side_of_the_caret_and_hash() {
    let mut menu = OptionsMenu::new([1, 2, 3], None);
    menu.open();
    menu.input(OptionsInput::Paste("123456".into()));
    menu.input(OptionsInput::Home);
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::DeleteToStart);
    let editor = menu.state.as_ref().unwrap().editor.as_ref().unwrap();
    assert_eq!((&*editor.text, editor.cursor), ("#3456", 1));
    menu.input(OptionsInput::Cycle(1));
    menu.input(OptionsInput::DeleteToEnd);
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#3"
    );
    menu.input(OptionsInput::End);
    menu.input(OptionsInput::DeleteToStart);
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#"
    );
}

#[test]
fn confirming_custom_validates_before_saving() {
    let mut menu = OptionsMenu::new([1, 2, 3], None);
    menu.open();
    menu.input(OptionsInput::SelectAll);
    menu.input(OptionsInput::Character('a'));
    menu.input(OptionsInput::Confirm);
    assert!(menu.state.as_ref().unwrap().error.is_some());
    menu.input(OptionsInput::Paste("ef4444".into()));
    menu.input(OptionsInput::Confirm);
    assert!(
        menu.state
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .starts_with("Not saved:")
    );
    assert_eq!(menu.color, [1, 2, 3]);
}

#[test]
fn mouse_arrows_can_leave_custom_while_keyboard_arrows_edit_it() {
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, None);
    menu.open();
    menu.action(OptionsAction::Cycle(-1));
    assert_eq!(menu.selected, CUSTOM);
    menu.input(OptionsInput::Character('a'));
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.selected, CUSTOM);
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().cursor,
        1
    );
    // Default needs no file write when there is no override, but a save path is
    // still required. A failed save leaves the editor available for correction.
    menu.action(OptionsAction::Cycle(1));
    assert!(menu.state.as_ref().unwrap().error.is_some());
    assert_eq!(menu.selected, CUSTOM);
}

#[test]
fn empty_custom_keeps_cycling_until_a_digit_is_typed() {
    let root = test_dir("empty-custom");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path));
    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#"
    );
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.selected, CUSTOM - 1);
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.selected, CUSTOM);
    menu.input(OptionsInput::Character('a'));
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.selected, CUSTOM);
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().cursor,
        1
    );
    menu.input(OptionsInput::Delete);
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(menu.selected, 0);
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(
        menu.state.as_ref().unwrap().editor.as_ref().unwrap().text,
        "#"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn playback_controls_autohide_saves_presets_never_and_custom_seconds() {
    let root = test_dir("autohide");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(-1));
    assert_eq!(
        menu.selected_setting,
        OptionsSetting::PlaybackControlsAutohide
    );
    menu.input(OptionsInput::Cycle(-2));
    assert_eq!(menu.playback_controls_autohide, 0);
    assert_eq!(
        Config::load(Some(&path))
            .unwrap()
            .playback_controls_autohide,
        0
    );

    menu.input(OptionsInput::Cycle(1));
    let editor = menu
        .state
        .as_ref()
        .unwrap()
        .seconds_editor
        .as_ref()
        .unwrap();
    assert!(editor.text.is_empty());
    for digit in ['9', '9', '9', '9', '9'] {
        menu.input(OptionsInput::Character(digit));
    }
    menu.input(OptionsInput::Confirm);
    assert_eq!(menu.playback_controls_autohide, 9999);
    assert!(menu.state.is_none());
    assert_eq!(
        Config::load(Some(&path))
            .unwrap()
            .playback_controls_autohide,
        9999
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn autohide_custom_editor_keeps_quit_shortcuts_available() {
    let root = test_dir("autohide-quit");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(-1));
    menu.input(OptionsInput::Cycle(-1));
    menu.input(OptionsInput::Paste("17".into()));
    assert_eq!(
        menu.input(OptionsInput::Character('q')),
        Some(PlaybackOutcome::Quit)
    );
    assert_eq!(
        Config::load(Some(&path))
            .unwrap()
            .playback_controls_autohide,
        17
    );
    assert_eq!(
        menu.input(OptionsInput::Character('Q')),
        Some(PlaybackOutcome::QuitWithoutSaving)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn navigating_away_saves_a_valid_custom_autohide_draft() {
    let root = test_dir("autohide-navigate");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(-1));
    menu.input(OptionsInput::Cycle(-1));
    menu.input(OptionsInput::Paste("42".into()));
    menu.input(OptionsInput::Navigate(1));

    assert_eq!(menu.selected_setting, OptionsSetting::AccentColor);
    assert_eq!(menu.playback_controls_autohide, 42);
    assert_eq!(
        Config::load(Some(&path))
            .unwrap()
            .playback_controls_autohide,
        42
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remembered_custom_autohide_survives_presets_and_restart() {
    let root = test_dir("remembered-autohide");
    let path = root.join("config.toml");
    let mut menu = OptionsMenu::new(DEFAULT_ACCENT_COLOR, Some(path.clone()));
    menu.open();
    menu.input(OptionsInput::Navigate(-1));
    menu.input(OptionsInput::Cycle(-1));
    menu.input(OptionsInput::Paste("42".into()));
    menu.input(OptionsInput::Confirm);

    menu.open();
    menu.input(OptionsInput::Cycle(-1));
    assert_eq!(menu.playback_controls_autohide, 0);
    menu.input(OptionsInput::Cycle(1));
    assert_eq!(
        menu.state
            .as_ref()
            .and_then(|state| state.seconds_editor.as_ref())
            .map(|editor| editor.text.as_str()),
        Some("42")
    );

    let config = Config::load(Some(&path)).unwrap();
    assert_eq!(config.playback_controls_autohide, 0);
    assert_eq!(config.custom_playback_controls_autohide, Some(42));
    let restarted = OptionsMenu::with_appearance(
        DEFAULT_ACCENT_COLOR,
        crate::config::DEFAULT_PANEL_OPACITY,
        crate::config::DEFAULT_BACKGROUND_BLUR,
        config.playback_controls_autohide,
        config.custom_playback_controls_autohide,
        Some(path),
    );
    assert_eq!(restarted.custom_playback_controls_autohide, Some(42));
    std::fs::remove_dir_all(root).unwrap();
}
