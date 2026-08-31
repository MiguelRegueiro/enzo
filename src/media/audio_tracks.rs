use std::path::Path;

use crate::subtitle::{language_display_name, normalize_language_tag};

use super::{
    ffi_support::{ErrorBuffer, path_cstring},
    media_ffi::{EnzoAudioTrackInfo, enzo_audio_tracks_free, enzo_probe_audio_tracks},
    metadata_display::{audio_channel_label, codec_display_name, fixed_info_text, format_rate},
};

const AUDIO_OUTPUT_SUMMARY: &str = "PCM S16 · Stereo · 48 kHz";

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

fn audio_track_label(track: &AudioTrackProbe, fallback_index: usize) -> String {
    let mut label = track
        .language
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("Track {}", fallback_index + 1));
    let title = track.title.as_deref();
    if let Some(title) = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .filter(|title| !title_mentions(Some(title), &label))
    {
        label.push_str(" (");
        label.push_str(title);
        label.push(')');
    }
    if let Some(channels) = audio_channel_label(track.channels, track.channel_layout.as_deref()) {
        label.push(' ');
        label.push_str(&channels);
    }
    if track.default {
        label.push_str(" [Default]");
    }
    if let Some(codec) = track.codec.as_deref() {
        label.push_str(" [");
        label.push_str(&codec_display_name(codec));
        label.push(']');
    }
    label
}

fn title_mentions(title: Option<&str>, value: &str) -> bool {
    title.is_some_and(|title| {
        title
            .to_ascii_lowercase()
            .contains(&value.to_ascii_lowercase())
    })
}

fn normalize_audio_language(value: &str) -> Option<String> {
    normalize_language_tag(value).map(|language| language_display_name(&language))
}

#[cfg(test)]
#[path = "tests/audio_tracks.rs"]
mod tests;
