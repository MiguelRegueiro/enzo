use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after the Unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("enzo-config-{label}-{unique}"))
}

#[test]
fn explicit_config_path_is_loaded() {
    let root = temp_path("explicit");
    let path = root.join("custom.toml");
    fs::create_dir_all(&root).expect("config directory should be created");
    fs::write(
        &path,
        "volume_max = 180\nresume = false\nautoplay_next = false\naccent_color = \"#102030\"\n",
    )
    .expect("config should be written");

    let config = Config::load(Some(&path)).expect("explicit config should load");

    assert_eq!(
        config,
        Config {
            volume_max: 180,
            resume: false,
            autoplay_next: false,
            accent_color: [0x10, 0x20, 0x30],
        }
    );
    fs::remove_dir_all(root).expect("config directory should be removed");
}

#[test]
fn missing_explicit_config_path_is_an_error() {
    let path = temp_path("missing").join("config.toml");

    let error = Config::load(Some(&path)).expect_err("missing explicit config should fail");

    assert!(
        error
            .to_string()
            .contains(&format!("failed to read config from {}", path.display()))
    );
}

#[test]
fn invalid_config_falls_back_to_defaults() {
    let root = temp_path("invalid");
    let path = root.join("config.toml");
    fs::create_dir_all(&root).expect("config directory should be created");
    fs::write(&path, "volume_max = 20\n").expect("config should be written");

    let config = Config::load(Some(&path)).expect("invalid config should fall back");

    assert_eq!(config, Config::default());
    fs::remove_dir_all(root).expect("config directory should be removed");
}

#[cfg(unix)]
#[test]
fn xdg_config_home_takes_precedence() {
    assert_eq!(
        platform_config_home(
            Some(Path::new("/tmp/custom-config")),
            Some(Path::new("/home/paco")),
        ),
        Some(PathBuf::from("/tmp/custom-config"))
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn unix_config_home_defaults_to_dot_config() {
    assert_eq!(
        platform_config_home(None, Some(Path::new("/home/paco"))),
        Some(PathBuf::from("/home/paco/.config"))
    );
}
