use std::{fs, path::Path};

use crate::media::VideoInfo;

pub(super) fn file_info_summary(path: &Path, source: &VideoInfo) -> String {
    let mut parts = Vec::new();
    let path_text = path.to_string_lossy();
    if let Some((scheme, _)) = path_text.split_once("://")
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        parts.push(scheme.to_ascii_uppercase());
    }
    if let Some(container) = source.container.as_deref() {
        parts.push(container_display_name(container));
    }
    if let Ok(metadata) = fs::metadata(path)
        && metadata.is_file()
    {
        parts.push(format_file_size(metadata.len()));
    }
    if parts.is_empty() {
        "Unknown".to_string()
    } else {
        parts.join(" · ")
    }
}

pub(super) fn container_display_name(container: &str) -> String {
    match container.split(',').next().unwrap_or(container) {
        "matroska" => "Matroska".to_string(),
        "mov" => "MP4 / MOV".to_string(),
        "mpegts" => "MPEG-TS".to_string(),
        "mpeg" => "MPEG".to_string(),
        "avi" => "AVI".to_string(),
        "flv" => "FLV".to_string(),
        "ogg" => "Ogg".to_string(),
        "hls" => "HLS".to_string(),
        value => value.to_ascii_uppercase(),
    }
}

pub(super) fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
