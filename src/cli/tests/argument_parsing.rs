use super::*;

fn run_options(args: Vec<OsString>) -> Options {
    run_options_with_config(args, Config::default())
}

fn run_options_with_config(args: Vec<OsString>, config: Config) -> Options {
    match parse_args_with_config_loader(args.into_iter(), |_| Ok(config))
        .expect("args should parse")
    {
        Action::Run(options) => options,
        Action::Help | Action::Version => panic!("expected run options"),
    }
}

#[test]
fn joins_shell_split_path_parts() {
    let path = join_positionals(vec![
        OsString::from("/tmp/La"),
        OsString::from("fascinante"),
        OsString::from("historia.mp4"),
    ])
    .expect("path should be reconstructed");

    assert_eq!(path, PathBuf::from("/tmp/La fascinante historia.mp4"));
}

#[test]
fn parse_args_accepts_launcher_without_path() {
    let config = run_options(Vec::new());

    assert_eq!(config.path, None);
    assert!(!config.force);
    assert_eq!(config.force_media_title, None);
    assert_eq!(config.sub_file, None);
    assert_eq!(config.volume_max, 100);
    assert!(config.resume_enabled);
    assert!(config.autoplay_next);
    assert_eq!(config.accent_color, Config::default().accent_color);
    assert!(!config.clear_resume);
}

#[test]
fn parse_args_supports_volume_max_forms() {
    let separate = run_options(vec![OsString::from("--volume-max"), OsString::from("180")]);
    assert_eq!(separate.volume_max, 180);

    let joined = run_options(vec![OsString::from("--volume-max=250")]);
    assert_eq!(joined.volume_max, 250);
}

#[test]
fn config_values_supply_playback_defaults() {
    let options = run_options_with_config(
        Vec::new(),
        Config {
            volume_max: 220,
            resume: false,
            autoplay_next: false,
            accent_color: [1, 2, 3],
        },
    );

    assert_eq!(options.volume_max, 220);
    assert!(!options.resume_enabled);
    assert!(!options.autoplay_next);
    assert_eq!(options.accent_color, [1, 2, 3]);
}

#[test]
fn command_line_values_override_config() {
    let options = run_options_with_config(
        vec![
            OsString::from("--volume-max=180"),
            OsString::from("--resume"),
            OsString::from("--autoplay-next"),
        ],
        Config {
            volume_max: 220,
            resume: false,
            autoplay_next: false,
            accent_color: [1, 2, 3],
        },
    );

    assert_eq!(options.volume_max, 180);
    assert!(options.resume_enabled);
    assert!(options.autoplay_next);
}

#[test]
fn custom_config_path_is_forwarded_to_loader() {
    let path = PathBuf::from("/tmp/custom-enzo.toml");
    let action = parse_args_with_config_loader(
        vec![OsString::from("--config"), path.clone().into_os_string()].into_iter(),
        |actual| {
            assert_eq!(actual, Some(path.as_path()));
            Ok(Config::default())
        },
    )
    .expect("custom config path should parse");

    assert!(matches!(action, Action::Run(_)));
}

#[test]
fn parse_args_rejects_invalid_volume_max() {
    for value in ["99", "1001", "loud"] {
        let error = parse_args(vec![OsString::from(format!("--volume-max={value}"))].into_iter())
            .err()
            .expect("invalid maximum volume should fail");
        assert!(error.to_string().contains("--volume-max"));
    }
}

#[test]
fn parse_args_supports_resume_controls() {
    let no_resume = run_options(vec![OsString::from("--no-resume")]);
    assert!(!no_resume.resume_enabled);
    assert!(!no_resume.clear_resume);

    let resume = run_options_with_config(
        vec![OsString::from("--resume")],
        Config {
            resume: false,
            ..Config::default()
        },
    );
    assert!(resume.resume_enabled);

    let clear = run_options(vec![OsString::from("--clear-resume")]);
    assert!(clear.clear_resume);
    assert!(clear.path.is_none());
}

#[test]
fn parse_args_supports_autoplay_controls() {
    let config = run_options(vec![OsString::from("--no-autoplay-next")]);

    assert!(!config.autoplay_next);
    assert!(config.resume_enabled);

    let config = run_options_with_config(
        vec![OsString::from("--autoplay-next")],
        Config {
            autoplay_next: false,
            ..Config::default()
        },
    );
    assert!(config.autoplay_next);
}

#[test]
fn parse_args_recognizes_help_and_version() {
    assert!(matches!(
        parse_args(vec![OsString::from("--help")].into_iter()),
        Ok(Action::Help)
    ));
    assert!(matches!(
        parse_args(vec![OsString::from("-V")].into_iter()),
        Ok(Action::Version)
    ));
}

#[test]
fn parse_args_accepts_remote_url() {
    let config = run_options(vec![OsString::from("https://example.com/video.mp4")]);

    assert_eq!(
        config.path,
        Some(PathBuf::from("https://example.com/video.mp4"))
    );
    assert_eq!(config.sub_file, None);
}

#[test]
fn parse_args_accepts_media_title_forms() {
    let separate = run_options(vec![
        OsString::from("--force-media-title"),
        OsString::from("Frieren Episode 1"),
        OsString::from("https://example.com/index.m3u8"),
    ]);
    assert_eq!(
        separate.force_media_title.as_deref(),
        Some("Frieren Episode 1")
    );

    let joined = run_options(vec![
        OsString::from("--force-media-title=葬送のフリーレン Episode 1"),
        OsString::from("https://example.com/index.m3u8"),
    ]);
    assert_eq!(
        joined.force_media_title.as_deref(),
        Some("葬送のフリーレン Episode 1")
    );
}

#[test]
fn parse_args_rejects_media_title_without_a_value() {
    let error = parse_args(vec![OsString::from("--force-media-title")].into_iter())
        .err()
        .expect("missing title should fail");

    assert!(error.to_string().contains("requires a title"));
}

#[test]
fn parse_args_accepts_sub_file() {
    let temp_dir =
        std::env::temp_dir().join(format!("enzo-app-subtitle-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir(&temp_dir).expect("temp dir should be created");
    let sub_file = temp_dir.join("movie.srt");
    std::fs::write(&sub_file, "").expect("subtitle should be written");

    let config = run_options(vec![
        OsString::from("--sub-file"),
        sub_file.clone().into_os_string(),
        OsString::from("https://example.com/video.mp4"),
    ]);

    assert_eq!(
        config.path,
        Some(PathBuf::from("https://example.com/video.mp4"))
    );
    assert_eq!(config.sub_file, Some(sub_file));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
