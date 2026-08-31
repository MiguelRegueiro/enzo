use std::ffi::c_char;

pub(super) fn fixed_info_text<const N: usize>(value: &[c_char; N]) -> Option<String> {
    let bytes = value
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect::<Vec<_>>();
    non_empty(&String::from_utf8_lossy(&bytes))
}

pub(super) fn codec_display_name(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "h264" => "H.264".to_string(),
        "hevc" => "HEVC".to_string(),
        "av1" => "AV1".to_string(),
        "vp9" => "VP9".to_string(),
        "aac" => "AAC".to_string(),
        "ac3" => "AC-3".to_string(),
        "eac3" => "E-AC-3".to_string(),
        "dts" => "DTS".to_string(),
        "flac" => "FLAC".to_string(),
        "opus" => "Opus".to_string(),
        other => other.to_uppercase(),
    }
}

pub(super) fn format_rate(value: f64) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else if value >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(super) fn audio_channel_label(channels: Option<u32>, layout: Option<&str>) -> Option<String> {
    if let Some(layout) = layout.filter(|layout| !layout.is_empty() && *layout != "unknown") {
        let layout = layout.replace("(side)", "").replace(['(', ')'], " ");
        return Some(match layout.trim() {
            "mono" => "Mono".to_string(),
            "stereo" => "Stereo".to_string(),
            other => other.split_whitespace().collect::<Vec<_>>().join(" "),
        });
    }
    match channels {
        Some(1) => Some("Mono".to_string()),
        Some(2) => Some("Stereo".to_string()),
        Some(value) => Some(format!("{value}ch")),
        None => None,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "N/A").then(|| value.to_string())
}
