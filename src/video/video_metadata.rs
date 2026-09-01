use std::{path::Path, time::Duration};

use anyhow::{Result, bail};

use crate::decoder_backend::{
    backend_bindings::{EnzoVideoInfo, HDR_HLG, HDR_PQ, INFO_TEXT_LEN, enzo_probe_video},
    backend_support::{ErrorBuffer, path_cstring},
    metadata_display::{codec_display_name, fixed_info_text, format_rate},
};

const MAX_PLAYBACK_FPS: f64 = 30.0;

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

#[cfg(test)]
#[path = "tests/video_metadata.rs"]
mod tests;
