use super::*;

#[test]
fn defaults_match_existing_playback_behavior() {
    assert_eq!(
        Config::default(),
        Config {
            volume_max: 100,
            resume: true,
            autoplay_next: true,
            accent_color: DEFAULT_ACCENT_COLOR,
        }
    );
}

#[test]
fn partial_config_overrides_defaults() {
    let config = Config::from_str("volume_max = 220\nresume = false\naccent_color = \"#1aB2c3\"\n")
        .expect("valid config should parse");

    assert_eq!(config.volume_max, 220);
    assert!(!config.resume);
    assert!(config.autoplay_next);
    assert_eq!(config.accent_color, [0x1a, 0xb2, 0xc3]);
}

#[test]
fn config_rejects_invalid_values_and_unknown_keys() {
    for contents in [
        "volume_max = 99\n",
        "volume_max = 1001\n",
        "resum = true\n",
        "accent_color = \"ef4444\"\n",
        "accent_color = \"#abcd\"\n",
        "accent_color = \"#gg0000\"\n",
        "accent_color = #ef4444\n",
    ] {
        assert!(Config::from_str(contents).is_err(), "contents: {contents}");
    }
}
