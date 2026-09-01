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
fn volume_keys_map_to_two_percent_steps() {
    assert_eq!(volume_steps_for_key(&KeyCode::Char('9')), Some(-1));
    assert_eq!(volume_steps_for_key(&KeyCode::Char('0')), Some(1));
    assert_eq!(volume_steps_for_key(&KeyCode::Char('m')), None);
}

#[test]
fn passive_mouse_and_wheel_input_do_not_interrupt_keyboard_seek() {
    assert!(!PlaybackMouse::Move { column: 1, row: 1 }.interrupts_keyboard_seek());
    assert!(PlaybackMouse::Down { column: 1, row: 1 }.interrupts_keyboard_seek());
    assert!(PlaybackMouse::Drag { column: 1, row: 1 }.interrupts_keyboard_seek());
    assert!(PlaybackMouse::Up { column: 1, row: 1 }.interrupts_keyboard_seek());
    assert!(!PlaybackMouse::ScrollUp.interrupts_keyboard_seek());
    assert!(!PlaybackMouse::ScrollDown.interrupts_keyboard_seek());
}

#[test]
fn playback_keys_map_to_commands() {
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('a')),
        PlaybackCommand::ToggleAudioPicker
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('s')),
        PlaybackCommand::ToggleSubtitlePicker
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('p')),
        PlaybackCommand::TogglePlaylistMenu
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('v')),
        PlaybackCommand::ToggleSubtitles
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Enter),
        PlaybackCommand::ConfirmPicker
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::PageUp),
        PlaybackCommand::PlaylistPrevious
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::PageDown),
        PlaybackCommand::PlaylistNext
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Home),
        PlaybackCommand::PlaylistFirst
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::End),
        PlaybackCommand::PlaylistLast
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('[')),
        PlaybackCommand::None
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char(']')),
        PlaybackCommand::None
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('Q')),
        PlaybackCommand::QuitWithoutSaving
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Char('?')),
        PlaybackCommand::ToggleHelp
    );
    assert_eq!(
        playback_command_for_key(&KeyCode::Esc),
        PlaybackCommand::CloseTransientUi
    );
}
