use std::{ffi::c_char, path::Path, time::Duration};

use anyhow::{Result, bail};

use super::{
    ffi::{
        EnzoAudioTrackInfo, EnzoSubtitleStreamInfo, EnzoVideoInfo, HDR_HLG, HDR_PQ, INFO_TEXT_LEN,
        enzo_audio_tracks_free, enzo_probe_audio_tracks, enzo_probe_subtitle_streams,
        enzo_probe_video, enzo_subtitle_streams_free,
    },
    native::{ErrorBuffer, path_cstring},
};

const MAX_PLAYBACK_FPS: f64 = 30.0;
const AUDIO_OUTPUT_SUMMARY: &str = "PCM S16 · Stereo · 48 kHz";

#[derive(Clone, Debug)]
pub(crate) struct VideoInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: f64,
    pub(crate) source_fps: f64,
    pub(crate) duration: Option<Duration>,
    pub(crate) has_audio: bool,
    pub(crate) seekable: bool,
    pub(crate) container: Option<String>,
    codec: Option<String>,
    profile: Option<String>,
    hdr: Option<&'static str>,
}

pub(crate) fn probe_video(path: &Path) -> Result<VideoInfo> {
    let path = path_cstring(path)?;
    let mut info = EnzoVideoInfo {
        width: 0,
        height: 0,
        fps: 0.0,
        duration: 0.0,
        has_audio: 0,
        seekable: 0,
        codec: [0; INFO_TEXT_LEN],
        profile: [0; INFO_TEXT_LEN],
        container: [0; INFO_TEXT_LEN],
        hdr: 0,
    };
    let mut error = ErrorBuffer::new();

    let status =
        unsafe { enzo_probe_video(path.as_ptr(), &mut info, error.as_mut_ptr(), error.len()) };
    if status < 0 {
        bail!("{}", error.message("failed to inspect video"));
    }

    let source_fps = info
        .fps
        .is_finite()
        .then_some(info.fps)
        .filter(|fps| *fps > 0.0)
        .unwrap_or(30.0);
    Ok(VideoInfo {
        width: info.width,
        height: info.height,
        fps: source_fps.min(MAX_PLAYBACK_FPS),
        source_fps,
        duration: info
            .duration
            .is_finite()
            .then_some(info.duration)
            .filter(|duration| *duration > 0.0)
            .map(Duration::from_secs_f64),
        has_audio: info.has_audio != 0,
        seekable: info.seekable != 0,
        container: fixed_info_text(&info.container),
        codec: fixed_info_text(&info.codec),
        profile: fixed_info_text(&info.profile),
        hdr: match info.hdr {
            HDR_PQ => Some("HDR (PQ)"),
            HDR_HLG => Some("HDR (HLG)"),
            _ => None,
        },
    })
}

impl VideoInfo {
    pub(crate) fn source_summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(codec) = self.codec.as_deref() {
            parts.push(codec_display_name(codec));
        }
        if let Some(profile) = self.profile.as_deref() {
            parts.push(profile.to_string());
        }
        parts.push(format!("{}×{}", self.width, self.height));
        parts.push(format!("{} fps", format_rate(self.source_fps)));
        if let Some(hdr) = self.hdr {
            parts.push(hdr.to_string());
        }
        parts.join(" · ")
    }
}

fn fixed_info_text<const N: usize>(value: &[c_char; N]) -> Option<String> {
    let bytes = value
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect::<Vec<_>>();
    non_empty(&String::from_utf8_lossy(&bytes))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AudioTrack {
    stream_index: usize,
    label: String,
    codec: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    sample_rate: Option<u32>,
}

impl AudioTrack {
    pub(crate) fn default_track() -> Self {
        Self {
            stream_index: usize::MAX,
            label: "Default".to_string(),
            codec: None,
            channels: None,
            channel_layout: None,
            sample_rate: None,
        }
    }

    pub(crate) fn stream_index(&self) -> usize {
        self.stream_index
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn playback_summary(&self) -> String {
        let mut source = Vec::new();
        if let Some(codec) = self.codec.as_deref() {
            source.push(codec_display_name(codec));
        }
        if let Some(channels) = audio_channel_label(self.channels, self.channel_layout.as_deref()) {
            source.push(channels);
        }
        if let Some(sample_rate) = self.sample_rate {
            source.push(format!(
                "{} kHz",
                format_rate(f64::from(sample_rate) / 1_000.0)
            ));
        }

        if source.is_empty() {
            format!("Output: {AUDIO_OUTPUT_SUMMARY}")
        } else {
            format!(
                "Source: {} | Output: {AUDIO_OUTPUT_SUMMARY}",
                source.join(" · ")
            )
        }
    }
}

pub(crate) fn load_audio_tracks(path: &Path) -> Vec<AudioTrack> {
    let Ok(path) = path_cstring(path) else {
        return Vec::new();
    };
    let mut tracks = std::ptr::null_mut();
    let mut count = 0_usize;
    let mut error = ErrorBuffer::new();
    let status = unsafe {
        enzo_probe_audio_tracks(
            path.as_ptr(),
            &mut tracks,
            &mut count,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status < 0 || count == 0 {
        return Vec::new();
    }

    let tracks = NativeAudioTrackList { tracks, count };
    tracks
        .as_slice()
        .iter()
        .enumerate()
        .filter_map(|(fallback, track)| {
            audio_track_from_probe(
                AudioTrackProbe {
                    stream_index: usize::try_from(track.stream_index).ok(),
                    codec: fixed_info_text(&track.codec),
                    language: fixed_info_text(&track.language)
                        .as_deref()
                        .and_then(normalize_audio_language),
                    title: fixed_info_text(&track.title),
                    channels: u32::try_from(track.channels)
                        .ok()
                        .filter(|value| *value > 0),
                    channel_layout: fixed_info_text(&track.channel_layout),
                    sample_rate: u32::try_from(track.sample_rate)
                        .ok()
                        .filter(|value| *value > 0),
                    default: track.is_default != 0,
                },
                fallback,
            )
        })
        .collect()
}

struct NativeAudioTrackList {
    tracks: *mut EnzoAudioTrackInfo,
    count: usize,
}

impl NativeAudioTrackList {
    fn as_slice(&self) -> &[EnzoAudioTrackInfo] {
        if self.tracks.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.tracks, self.count) }
        }
    }
}

impl Drop for NativeAudioTrackList {
    fn drop(&mut self) {
        unsafe {
            enzo_audio_tracks_free(self.tracks);
        }
    }
}

#[derive(Default)]
struct AudioTrackProbe {
    stream_index: Option<usize>,
    codec: Option<String>,
    language: Option<String>,
    title: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    sample_rate: Option<u32>,
    default: bool,
}

fn audio_track_from_probe(probe: AudioTrackProbe, fallback_index: usize) -> Option<AudioTrack> {
    let stream_index = probe.stream_index?;
    Some(AudioTrack {
        stream_index,
        label: audio_track_label(&probe, fallback_index),
        codec: probe.codec,
        channels: probe.channels,
        channel_layout: probe.channel_layout,
        sample_rate: probe.sample_rate,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SubtitleStreamInfo {
    pub(crate) subtitle_index: usize,
    pub(crate) codec: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) default: bool,
    pub(crate) forced: bool,
}

pub(crate) fn load_subtitle_streams(path: &Path) -> Vec<SubtitleStreamInfo> {
    let text = path.as_os_str().to_string_lossy();
    if text.contains("://") {
        return Vec::new();
    }
    let Ok(path) = path_cstring(path) else {
        return Vec::new();
    };
    let mut streams = std::ptr::null_mut();
    let mut count = 0_usize;
    let mut error = ErrorBuffer::new();
    let status = unsafe {
        enzo_probe_subtitle_streams(
            path.as_ptr(),
            &mut streams,
            &mut count,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status < 0 || count == 0 {
        return Vec::new();
    }

    let streams = NativeSubtitleStreamList { streams, count };
    streams
        .as_slice()
        .iter()
        .filter_map(|stream| {
            Some(SubtitleStreamInfo {
                subtitle_index: usize::try_from(stream.subtitle_index).ok()?,
                codec: fixed_info_text(&stream.codec),
                language: fixed_info_text(&stream.language),
                title: fixed_info_text(&stream.title),
                default: stream.is_default != 0,
                forced: stream.is_forced != 0,
            })
        })
        .collect()
}

struct NativeSubtitleStreamList {
    streams: *mut EnzoSubtitleStreamInfo,
    count: usize,
}

impl NativeSubtitleStreamList {
    fn as_slice(&self) -> &[EnzoSubtitleStreamInfo] {
        if self.streams.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.streams, self.count) }
        }
    }
}

impl Drop for NativeSubtitleStreamList {
    fn drop(&mut self) {
        unsafe {
            enzo_subtitle_streams_free(self.streams);
        }
    }
}

fn codec_display_name(codec: &str) -> String {
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

fn format_rate(value: f64) -> String {
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

fn audio_track_label(track: &AudioTrackProbe, fallback_index: usize) -> String {
    let mut parts = Vec::<String>::new();
    let title = track.title.as_deref();
    if let Some(language) = track
        .language
        .as_deref()
        .filter(|language| !title_mentions(title, language))
    {
        parts.push(language.to_string());
    }
    if let Some(title) =
        title.filter(|title| !parts.iter().any(|part| part.eq_ignore_ascii_case(title)))
    {
        parts.push(title.to_string());
    }
    if let Some(channels) = audio_channel_label(track.channels, track.channel_layout.as_deref())
        .filter(|channels| !title_mentions_channel(title, channels, track.channels))
    {
        parts.push(channels);
    }
    if let Some(codec) = track.codec.as_deref() {
        parts.push(codec.to_uppercase());
    }
    if track.default {
        parts.push("Default".to_string());
    }
    if parts.is_empty() {
        format!("Track {}", fallback_index + 1)
    } else {
        parts.join(" — ")
    }
}

fn title_mentions(title: Option<&str>, value: &str) -> bool {
    title.is_some_and(|title| {
        title
            .to_ascii_lowercase()
            .contains(&value.to_ascii_lowercase())
    })
}

fn title_mentions_channel(title: Option<&str>, value: &str, channels: Option<u32>) -> bool {
    if title_mentions(title, value) {
        return true;
    }
    let Some(title) = title.map(str::to_ascii_lowercase) else {
        return false;
    };
    match channels {
        Some(1) => title.contains("1.0") || title.contains("mono"),
        Some(2) => title.contains("2.0") || title.contains("stereo"),
        Some(6) => title.contains("5.1"),
        Some(8) => title.contains("7.1"),
        Some(value) => title.contains(&format!("{value}ch")),
        None => false,
    }
}

fn audio_channel_label(channels: Option<u32>, layout: Option<&str>) -> Option<String> {
    if let Some(layout) = layout.filter(|layout| !layout.is_empty() && *layout != "unknown") {
        let layout = layout.replace(['(', ')'], " ");
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

fn normalize_audio_language(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "und" => None,
        "eng" | "en" => Some("English".to_string()),
        "jpn" | "ja" | "jp" => Some("Japanese".to_string()),
        "spa" | "es" => Some("Spanish".to_string()),
        other => Some(other.to_string()),
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "N/A").then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn formats_probed_audio_track_metadata() {
        let track = audio_track_from_probe(
            AudioTrackProbe {
                stream_index: Some(2),
                codec: Some("aac".to_string()),
                language: Some("ja".to_string()),
                title: Some("Japanese 5.1".to_string()),
                channels: Some(6),
                channel_layout: Some("5.1".to_string()),
                sample_rate: Some(48_000),
                default: true,
            },
            0,
        )
        .expect("audio track should parse");

        assert_eq!(track.stream_index(), 2);
        assert_eq!(track.label(), "Japanese 5.1 — AAC — Default");
        assert_eq!(
            track.playback_summary(),
            "Source: AAC · 5.1 · 48 kHz | Output: PCM S16 · Stereo · 48 kHz"
        );
    }

    #[test]
    fn audio_track_label_falls_back_to_track_number() {
        let track = audio_track_from_probe(
            AudioTrackProbe {
                stream_index: Some(7),
                ..AudioTrackProbe::default()
            },
            2,
        )
        .expect("audio track should parse");

        assert_eq!(track.stream_index(), 7);
        assert_eq!(track.label(), "Track 3");
        assert_eq!(
            track.playback_summary(),
            "Output: PCM S16 · Stereo · 48 kHz"
        );
    }

    #[test]
    fn native_probe_reads_audio_track_metadata() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media = std::env::temp_dir().join(format!(
            "enzo-audio-track-probe-test-{}.mkv",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("anullsrc=channel_layout=5.1:sample_rate=48000")
            .args([
                "-t",
                "0.2",
                "-c:a",
                "flac",
                "-metadata:s:a:0",
                "language=jpn",
                "-metadata:s:a:0",
                "title=Japanese 5.1",
                "-disposition:a:0",
                "default",
            ])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let tracks = load_audio_tracks(&media);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].stream_index(), 0);
        assert_eq!(tracks[0].label(), "Japanese 5.1 — FLAC — Default");
        assert_eq!(
            tracks[0].playback_summary(),
            "Source: FLAC · 5.1 · 48 kHz | Output: PCM S16 · Stereo · 48 kHz"
        );

        let _ = std::fs::remove_file(media);
    }

    #[test]
    fn source_summary_keeps_original_frame_rate() {
        let info = VideoInfo {
            width: 3840,
            height: 2160,
            fps: 30.0,
            source_fps: 59.94,
            duration: None,
            has_audio: true,
            seekable: true,
            container: Some("matroska,webm".to_string()),
            codec: Some("hevc".to_string()),
            profile: Some("Main 10".to_string()),
            hdr: Some("HDR (PQ)"),
        };

        assert_eq!(
            info.source_summary(),
            "HEVC · Main 10 · 3840×2160 · 59.94 fps · HDR (PQ)"
        );
    }

    #[test]
    fn probe_preserves_source_rate_above_playback_cap_when_ffmpeg_is_available() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media =
            std::env::temp_dir().join(format!("enzo-media-info-test-{}.mkv", std::process::id()));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("color=size=16x16:duration=0.2:rate=60")
            .args(["-c:v", "ffv1"])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let info = probe_video(&media).expect("generated video should be probed");
        assert!((info.source_fps - 60.0).abs() < 0.01);
        assert_eq!(info.fps, MAX_PLAYBACK_FPS);
        assert_eq!(info.container.as_deref(), Some("matroska,webm"));
        let summary = info.source_summary();
        assert!(summary.starts_with("FFV1"));
        assert!(summary.contains("16×16 · 60 fps"));

        let _ = std::fs::remove_file(media);
    }
}
