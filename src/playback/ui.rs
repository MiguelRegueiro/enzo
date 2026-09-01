use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    media::VideoDecoder,
    overlay::{MediaInfo, MediaInfoState, OverlayState, PlaylistMenuState},
    playlist::PlaylistControls,
};

use super::{layout::CanvasFrame, subtitles::SubtitleCatalog, tracks::AudioCatalog};

const OVERLAY_VISIBLE_FOR: Duration = Duration::from_secs(2);
const STATUS_VISIBLE_FOR: Duration = Duration::from_secs(2);
pub(super) const MEDIA_INFO_VISIBLE_FOR: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub(super) struct StatusMessage {
    text: Arc<str>,
    visible_until: Instant,
}

pub(super) struct MediaInfoOverlay {
    content: MediaInfo,
    visible_until: Option<Instant>,
    pinned: bool,
}

impl MediaInfoOverlay {
    pub(super) fn new(content: MediaInfo, pinned: bool) -> Self {
        Self {
            content,
            visible_until: None,
            pinned,
        }
    }

    pub(super) fn show(&mut self, now: Instant) {
        self.visible_until = Some(now + MEDIA_INFO_VISIBLE_FOR);
    }

    pub(super) fn toggle(&mut self) {
        self.pinned = !self.pinned;
        self.visible_until = None;
    }

    pub(super) fn pinned(&self) -> bool {
        self.pinned
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
    pub(super) playlist_menu_open: bool,
    pub(super) playlist_menu_offset: usize,
    pub(super) playlist_menu_focus: Option<usize>,
    pub(super) playlist_current: usize,
    pub(super) playlist_labels: Arc<[Arc<str>]>,
    pub(super) audio_picker_open: bool,
    pub(super) audio_picker_offset: usize,
    pub(super) audio_picker_focus: Option<usize>,
    pub(super) subtitle_picker_open: bool,
    pub(super) subtitle_picker_offset: usize,
    pub(super) subtitle_picker_focus: Option<usize>,
    pub(super) help_visible: bool,
    pub(super) help_scroll_offset: usize,
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
        media_info_pinned: bool,
        playlist_current: usize,
        playlist_labels: Arc<[Arc<str>]>,
    ) -> Self {
        Self {
            playlist_menu_open: false,
            playlist_menu_offset: 0,
            playlist_menu_focus: None,
            playlist_current,
            playlist_labels,
            audio_picker_open: false,
            audio_picker_offset: 0,
            audio_picker_focus: None,
            subtitle_picker_open: false,
            subtitle_picker_offset: 0,
            subtitle_picker_focus: None,
            help_visible: false,
            help_scroll_offset: 0,
            overlay_visible_until: None,
            status_message,
            media_info: MediaInfoOverlay::new(media_info, media_info_pinned),
            media_title,
        }
    }

    pub(super) fn status(text: impl Into<Arc<str>>, now: Instant) -> StatusMessage {
        StatusMessage {
            text: text.into(),
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
        playlist_controls: PlaylistControls,
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
            self.status_message.as_ref(),
            playlist_controls,
            PlaylistMenuState {
                open: self.playlist_menu_open,
                current: self.playlist_current,
                scroll_offset: self.playlist_menu_offset,
                focus: self.playlist_menu_focus,
                labels: Arc::clone(&self.playlist_labels),
            },
            audio.is_available(),
            audio.selected(),
            self.audio_picker_open,
            self.audio_picker_offset,
            self.audio_picker_focus,
            audio.labels(),
            subtitles.is_available(),
            subtitles.selected(),
            self.subtitle_picker_open,
            self.subtitle_picker_offset,
            self.subtitle_picker_focus,
            subtitles.labels(),
            self.media_title.clone(),
            self.media_info
                .state(audio.selected(), canvas, decoder, paused, Instant::now()),
            self.help_visible,
            self.help_scroll_offset,
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
    status_message: Option<&StatusMessage>,
    playlist_controls: PlaylistControls,
    playlist: PlaylistMenuState,
    audio_available: bool,
    selected_audio: Option<usize>,
    audio_picker_open: bool,
    audio_picker_offset: usize,
    audio_picker_focus: Option<usize>,
    audio_labels: Arc<[Arc<str>]>,
    subtitles_available: bool,
    selected_subtitle: Option<usize>,
    subtitle_picker_open: bool,
    subtitle_picker_offset: usize,
    subtitle_picker_focus: Option<usize>,
    subtitle_labels: Arc<[Arc<str>]>,
    media_title: Arc<str>,
    media_info: Option<MediaInfoState>,
    help_visible: bool,
    help_scroll_offset: usize,
) -> OverlayState {
    let now = Instant::now();
    OverlayState {
        position: scrub_position.unwrap_or(position),
        duration,
        paused,
        visible: overlay_visible(paused, scrub_position.is_some(), visible_until, now)
            || audio_picker_open
            || subtitle_picker_open,
        playlist_previous_available: playlist_controls.previous_available,
        playlist_next_available: playlist_controls.next_available,
        playlist,
        audio_available,
        selected_audio,
        audio_picker_open,
        audio_picker_offset,
        audio_picker_focus,
        audio_labels,
        subtitles_available,
        selected_subtitle,
        subtitle_picker_open,
        subtitle_picker_offset,
        subtitle_picker_focus,
        subtitle_labels,
        status_message: status_message
            .filter(|message| now < message.visible_until)
            .map(|message| Arc::clone(&message.text)),
        media_title: Some(media_title),
        media_info,
        help_visible,
        help_scroll_offset,
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

pub(super) fn resolve_media_title(path: &Path, forced: Option<&str>) -> Arc<str> {
    if let Some(title) = forced.filter(|title| !title.is_empty()) {
        return Arc::from(title);
    }
    let text = path
        .file_name()
        .filter(|name| !name.is_empty())
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned();
    Arc::from(text)
}

pub(super) fn status_text(message: Option<&StatusMessage>, now: Instant) -> Option<&str> {
    message.and_then(|message| (now < message.visible_until).then_some(message.text.as_ref()))
}

pub(super) fn overlay_visible(
    paused: bool,
    scrubbing: bool,
    visible_until: Option<Instant>,
    now: Instant,
) -> bool {
    paused || scrubbing || visible_until.is_some_and(|until| now < until)
}

#[cfg(test)]
#[path = "tests/ui.rs"]
mod tests;
