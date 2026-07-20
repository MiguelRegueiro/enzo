use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    media::VideoDecoder,
    overlay::{MediaInfo, MediaInfoState, OverlayState},
};

use super::{layout::CanvasFrame, subtitles::SubtitleCatalog, tracks::AudioCatalog};

const OVERLAY_VISIBLE_FOR: Duration = Duration::from_secs(2);
const STATUS_VISIBLE_FOR: Duration = Duration::from_secs(2);
pub(super) const MEDIA_INFO_VISIBLE_FOR: Duration = Duration::from_secs(4);

#[derive(Clone, Copy)]
pub(super) struct StatusMessage {
    text: &'static str,
    visible_until: Instant,
}

pub(super) struct MediaInfoOverlay {
    content: MediaInfo,
    visible_until: Option<Instant>,
    pinned: bool,
}

impl MediaInfoOverlay {
    pub(super) fn new(content: MediaInfo) -> Self {
        Self {
            content,
            visible_until: None,
            pinned: false,
        }
    }

    pub(super) fn show(&mut self, now: Instant) {
        self.visible_until = Some(now + MEDIA_INFO_VISIBLE_FOR);
    }

    pub(super) fn toggle(&mut self) {
        self.pinned = !self.pinned;
        self.visible_until = None;
    }

    pub(super) fn visible(&self, now: Instant) -> bool {
        self.pinned || self.visible_until.is_some_and(|deadline| now < deadline)
    }

    fn state(
        &self,
        selected_audio: Option<usize>,
        canvas: CanvasFrame,
        decoder: &VideoDecoder,
        paused: bool,
        now: Instant,
    ) -> Option<MediaInfoState> {
        if !self.visible(now) {
            return None;
        }
        Some(MediaInfoState {
            info: self.content.clone(),
            selected_audio,
            display_width: canvas.video_width,
            display_height: canvas.video_height,
            display_paused: paused,
            display_fps: media_info_display_fps(paused, decoder.display_fps(now)),
        })
    }
}

pub(super) struct PlaybackUi {
    pub(super) audio_picker_open: bool,
    pub(super) audio_picker_offset: usize,
    pub(super) subtitle_picker_open: bool,
    pub(super) subtitle_picker_offset: usize,
    pub(super) overlay_visible_until: Option<Instant>,
    pub(super) status_message: Option<StatusMessage>,
    pub(super) media_info: MediaInfoOverlay,
    media_title: Arc<str>,
}

impl PlaybackUi {
    pub(super) fn new(
        media_title: Arc<str>,
        media_info: MediaInfo,
        status_message: Option<StatusMessage>,
    ) -> Self {
        Self {
            audio_picker_open: false,
            audio_picker_offset: 0,
            subtitle_picker_open: false,
            subtitle_picker_offset: 0,
            overlay_visible_until: None,
            status_message,
            media_info: MediaInfoOverlay::new(media_info),
            media_title,
        }
    }

    pub(super) fn status(text: &'static str, now: Instant) -> StatusMessage {
        StatusMessage {
            text,
            visible_until: now + STATUS_VISIBLE_FOR,
        }
    }

    pub(super) fn show_overlay(&mut self, now: Instant) {
        self.overlay_visible_until = Some(now + OVERLAY_VISIBLE_FOR);
    }

    pub(super) fn overlay_visible(&self, paused: bool, scrubbing: bool, now: Instant) -> bool {
        overlay_visible(paused, scrubbing, self.overlay_visible_until, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn state(
        &self,
        position: Duration,
        scrub_position: Option<Duration>,
        duration: Option<Duration>,
        paused: bool,
        audio: &AudioCatalog,
        subtitles: &SubtitleCatalog,
        canvas: CanvasFrame,
        decoder: &VideoDecoder,
    ) -> OverlayState {
        overlay_state(
            position,
            scrub_position,
            duration,
            paused,
            self.overlay_visible_until,
            self.status_message,
            audio.is_available(),
            audio.selected(),
            self.audio_picker_open,
            self.audio_picker_offset,
            audio.labels(),
            subtitles.is_available(),
            subtitles.selected(),
            self.subtitle_picker_open,
            self.subtitle_picker_offset,
            subtitles.labels(),
            self.media_title.clone(),
            self.media_info
                .state(audio.selected(), canvas, decoder, paused, Instant::now()),
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn overlay_state(
    position: Duration,
    scrub_position: Option<Duration>,
    duration: Option<Duration>,
    paused: bool,
    visible_until: Option<Instant>,
    status_message: Option<StatusMessage>,
    audio_available: bool,
    selected_audio: Option<usize>,
    audio_picker_open: bool,
    audio_picker_offset: usize,
    audio_labels: Arc<[Arc<str>]>,
    subtitles_available: bool,
    selected_subtitle: Option<usize>,
    subtitle_picker_open: bool,
    subtitle_picker_offset: usize,
    subtitle_labels: Arc<[Arc<str>]>,
    media_title: Arc<str>,
    media_info: Option<MediaInfoState>,
) -> OverlayState {
    let now = Instant::now();
    OverlayState {
        position: scrub_position.unwrap_or(position),
        duration,
        paused,
        visible: overlay_visible(paused, scrub_position.is_some(), visible_until, now)
            || audio_picker_open
            || subtitle_picker_open,
        audio_available,
        selected_audio,
        audio_picker_open,
        audio_picker_offset,
        audio_labels,
        subtitles_available,
        selected_subtitle,
        subtitle_picker_open,
        subtitle_picker_offset,
        subtitle_labels,
        status_message: status_text(status_message, now),
        media_title: Some(media_title),
        media_info,
    }
}

#[cfg(test)]
pub(super) fn media_info_fps_visible(state: &OverlayState) -> bool {
    state
        .media_info
        .as_ref()
        .is_some_and(|info| info.display_fps.is_some())
}

pub(super) fn media_info_display_fps(paused: bool, sampled_fps: Option<f64>) -> Option<f64> {
    (!paused).then_some(sampled_fps).flatten()
}

pub(super) fn media_title(path: &Path) -> Arc<str> {
    let text = path
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Arc::from(text)
}

pub(super) fn status_text(message: Option<StatusMessage>, now: Instant) -> Option<&'static str> {
    message.and_then(|message| (now < message.visible_until).then_some(message.text))
}

pub(super) fn overlay_visible(
    paused: bool,
    scrubbing: bool,
    visible_until: Option<Instant>,
    now: Instant,
) -> bool {
    paused || scrubbing || visible_until.is_some_and(|until| now < until)
}
