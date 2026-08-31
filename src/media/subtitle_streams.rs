use std::path::Path;

use super::{
    ffi_support::{ErrorBuffer, path_cstring},
    media_ffi::{EnzoSubtitleStreamInfo, enzo_probe_subtitle_streams, enzo_subtitle_streams_free},
    metadata_display::fixed_info_text,
};

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
