//! Overlay inputs, display metadata, and semantic interaction results.

use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct OverlayState {
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) paused: bool,
    pub(crate) visible: bool,
    pub(crate) audio_available: bool,
    pub(crate) selected_audio: Option<usize>,
    pub(crate) audio_picker_open: bool,
    pub(crate) audio_picker_offset: usize,
    pub(crate) audio_picker_focus: Option<usize>,
    pub(crate) audio_labels: Arc<[Arc<str>]>,
    pub(crate) subtitles_available: bool,
    pub(crate) selected_subtitle: Option<usize>,
    pub(crate) subtitle_picker_open: bool,
    pub(crate) subtitle_picker_offset: usize,
    pub(crate) subtitle_picker_focus: Option<usize>,
    pub(crate) subtitle_labels: Arc<[Arc<str>]>,
    pub(crate) status_message: Option<&'static str>,
    pub(crate) media_title: Option<Arc<str>>,
    pub(crate) media_info: Option<MediaInfoState>,
}

#[derive(Clone, Debug)]
pub(crate) struct MediaInfo {
    pub(super) file: Arc<str>,
    pub(super) video: Arc<str>,
    pub(super) audio: Arc<[Arc<str>]>,
}

impl MediaInfo {
    pub(crate) fn new(file: String, video: String, audio: Vec<String>) -> Self {
        Self {
            file: Arc::from(file),
            video: Arc::from(video),
            audio: audio
                .into_iter()
                .map(Arc::<str>::from)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MediaInfoState {
    pub(crate) info: MediaInfo,
    pub(crate) selected_audio: Option<usize>,
    pub(crate) display_width: u32,
    pub(crate) display_height: u32,
    pub(crate) display_paused: bool,
    pub(crate) display_fps: Option<f64>,
}

#[derive(Clone, Copy)]
pub(crate) struct OverlayHitContext {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) terminal_rows: u16,
    pub(crate) scale_percent: u32,
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) audio_available: bool,
    pub(crate) subtitles_available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AudioPickerAction {
    TogglePicker,
    SelectTrack(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubtitlePickerAction {
    TogglePicker,
    SelectOff,
    SelectTrack(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct OverlayHitPoint {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) cell: HitboxRect,
}

#[derive(Clone, Copy)]
pub(crate) struct HitboxRect {
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
}
