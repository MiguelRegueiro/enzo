use super::*;

#[test]
fn changing_accent_preserves_comments_and_other_settings() {
    let source =
        "# My config\nvolume_max = 300 # loud\n'accent_color' = '#4450ef' # blue\nresume = false\n";
    let updated = edit_accent_color(source, Some([1, 2, 3]), None).unwrap();
    assert_eq!(
        updated,
        "# My config\nvolume_max = 300 # loud\n'accent_color' = \"#010203\" # blue\nresume = false\n"
    );
    assert_eq!(Config::from_str(&updated).unwrap().accent_color, [1, 2, 3]);
}

#[test]
fn default_removes_override_and_invalid_config_is_not_replaced() {
    let updated =
        edit_accent_color("volume_max = 300\naccent_color = \"#4450ef\"\n", None, None).unwrap();
    assert_eq!(updated, "volume_max = 300\n");
    assert!(edit_accent_color("volume_max = 3", Some([1, 2, 3]), None).is_err());
    assert!(edit_accent_color("broken = [", None, None).is_err());
}

#[test]
fn remembered_custom_is_independent_of_active_accent_and_keeps_comments() {
    let source = "volume_max = 300 # keep\ncustom_accent_color = '#123456' # favorite\n";
    let saved =
        edit_accent_color(source, Some([0xab, 0xcd, 0xef]), Some([0xab, 0xcd, 0xef])).unwrap();
    assert!(saved.contains("custom_accent_color = \"#abcdef\" # favorite"));
    let preset = edit_accent_color(&saved, Some([0x44, 0x50, 0xef]), None).unwrap();
    let config = Config::from_str(&preset).unwrap();
    assert_eq!(config.accent_color, [0x44, 0x50, 0xef]);
    assert_eq!(config.custom_accent_color, Some([0xab, 0xcd, 0xef]));
    let default = edit_accent_color(&preset, None, None).unwrap();
    assert!(default.starts_with("volume_max = 300 # keep\n"));
    let config = Config::from_str(&default).unwrap();
    assert_eq!(config.accent_color, super::super::DEFAULT_ACCENT_COLOR);
    assert_eq!(config.custom_accent_color, Some([0xab, 0xcd, 0xef]));
}

#[test]
fn saves_to_custom_path_and_preserves_subsequent_external_edits() {
    let root = std::env::temp_dir().join(format!("enzo-config-save-{}", std::process::id()));
    let path = root.join("custom.toml");
    save_accent_color(&path, Some([1, 2, 3]), None).unwrap();
    fs::write(&path, "# edited outside Enzo\nvolume_max = 400\n").unwrap();
    save_accent_color(&path, Some([4, 5, 6]), None).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# edited outside Enzo\nvolume_max = 400\n"));
    assert_eq!(Config::load(Some(&path)).unwrap().accent_color, [4, 5, 6]);
    fs::write(&path, "broken = [").unwrap();
    assert!(save_accent_color(&path, None, None).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "broken = [");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn saves_through_symlinks_without_replacing_them() {
    let root = std::env::temp_dir().join(format!("enzo-config-link-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let target = root.join("actual.toml");
    let link = root.join("config.toml");
    fs::write(&target, "resume = false\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    save_accent_color(&link, Some([1, 2, 3]), None).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(Config::load(Some(&target)).unwrap().accent_color, [1, 2, 3]);
    fs::remove_dir_all(root).unwrap();
}
