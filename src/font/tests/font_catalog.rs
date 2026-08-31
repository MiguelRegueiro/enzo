use super::*;
use std::{
    fs::File,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn resolver_prefers_noto_over_other_fonts() {
    let root = temp_font_dir("prefers_noto");
    let random = root.join("Random-Regular.ttf");
    let noto = root.join("NotoSans-Regular.ttf");
    File::create(&random).expect("create random font");
    File::create(&noto).expect("create noto font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all(FontRole::Ui).next(),
        Some(noto.as_path())
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn resolver_falls_back_to_first_discovered_font() {
    let root = temp_font_dir("fallback_first");
    let zed = root.join("Zed-Regular.ttf");
    let alpha = root.join("Alpha-Regular.ttf");
    File::create(&zed).expect("create zed font");
    File::create(&alpha).expect("create alpha font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all(FontRole::Subtitle).next(),
        Some(alpha.as_path())
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn japanese_subtitles_prefer_japanese_cjk_fonts() {
    let root = temp_font_dir("japanese_subtitle_font");
    let latin = root.join("NotoSans-Regular.ttf");
    let chinese = root.join("NotoSansCJKSC-Regular.otf");
    let japanese = root.join("noto-cjk/NotoSansCJK-Regular.ttc");
    fs::create_dir_all(japanese.parent().expect("japanese parent")).expect("create japanese dir");
    File::create(&latin).expect("create latin font");
    File::create(&chinese).expect("create chinese font");
    File::create(&japanese).expect("create japanese font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all_for_language(FontRole::Subtitle, Some("ja"))[0],
        japanese
    );
    assert_eq!(
        system.resolve_all_for_language(FontRole::Ui, Some("ja"))[0],
        latin
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn chinese_subtitles_prefer_chinese_cjk_fonts() {
    let root = temp_font_dir("chinese_subtitle_font");
    let latin = root.join("NotoSans-Regular.ttf");
    let chinese = root.join("wenquanyi/wqy-zenhei.ttc");
    fs::create_dir_all(chinese.parent().expect("chinese parent")).expect("create chinese dir");
    File::create(&latin).expect("create latin font");
    File::create(&chinese).expect("create chinese font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all_for_language(FontRole::Subtitle, Some("zh-Hans"))[0],
        chinese
    );
    assert_eq!(
        system.resolve_all_for_language(FontRole::Ui, Some("zh-Hans"))[0],
        latin
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn arabic_subtitles_prefer_arabic_fonts() {
    let root = temp_font_dir("arabic_subtitle_font");
    let latin = root.join("NotoSans-Regular.ttf");
    let arabic = root.join("NotoSansArabic-Regular.ttf");
    File::create(&latin).expect("create latin font");
    File::create(&arabic).expect("create arabic font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all_for_language(FontRole::Subtitle, Some("ar"))[0],
        arabic
    );
    assert_eq!(
        system.resolve_all_for_language(FontRole::Ui, Some("ar"))[0],
        latin
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn ui_prefers_plain_fedora_noto_variable_font_over_script_specific_noto_fonts() {
    let root = temp_font_dir("fedora_noto_variable_font");
    let cjk = root
        .join("google-noto-sans-cjk-vf-fonts")
        .join("NotoSansCJK-VF.ttc");
    let arabic = root.join("google-noto-vf").join("NotoSansArabic[wght].ttf");
    let plain = root.join("google-noto-vf").join("NotoSans[wght].ttf");
    fs::create_dir_all(cjk.parent().expect("cjk parent")).expect("create cjk dir");
    fs::create_dir_all(plain.parent().expect("plain parent")).expect("create plain dir");
    File::create(&cjk).expect("create cjk font");
    File::create(&arabic).expect("create arabic font");
    File::create(&plain).expect("create plain font");

    let system = FontSystem::from_dirs([root.clone().into_os_string()]);

    assert_eq!(
        system.resolve_all(FontRole::Ui).next(),
        Some(plain.as_path())
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn accepts_common_unix_font_extensions() {
    assert!(is_font_file(Path::new("font.ttf")));
    assert!(is_font_file(Path::new("font.otf")));
    assert!(is_font_file(Path::new("font.ttc")));
    assert!(!is_font_file(Path::new("font.txt")));
}

#[test]
fn system_dirs_include_freebsd_font_location() {
    assert!(SYSTEM_FONT_DIRS.contains(&"/usr/local/share/fonts"));
}

fn temp_font_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = env::temp_dir().join(format!("enzo-font-system-{name}-{nonce}"));
    fs::create_dir_all(&dir).expect("create temp font dir");
    dir
}
