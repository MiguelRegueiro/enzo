use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, TryLockError},
    thread,
    time::Duration,
};

use crate::{
    font::FontRenderer,
    font_system::{FontRole, FontSystem},
};

use super::{
    bitmap::draw_bitmap_subtitle,
    text_render::{
        CachedSubtitleLayout, CachedTextOverlay, MAX_SUBTITLE_FALLBACK_FONTS,
        MAX_SUBTITLE_WIDTH_RATIO, build_text_overlay, composite_text_overlay, fallback_text_scale,
        open_first_font, prepare_subtitle_lines, subtitle_bottom_margin, subtitle_font_size,
    },
    track::SubtitleTrack,
};

const MAX_SUBTITLE_PREFETCH_CUES: usize = 8;
const MAX_READY_SUBTITLE_OVERLAYS: usize = 16;

#[cfg(test)]
#[path = "tests/renderer.rs"]
mod tests;

pub(crate) struct SubtitleRenderer {
    worker: Arc<TextOverlayWorkerState>,
    current_key: Option<TextOverlayRequestKey>,
    current_generation: u64,
    submitted_generation: Option<u64>,
    cached_overlay: Option<CachedTextOverlay>,
    ready_overlays: VecDeque<TextOverlayResult>,
    prefetch_initialized: bool,
    #[cfg(test)]
    request_submissions: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextOverlayRequestKey {
    lines: Vec<String>,
    canvas_width: u32,
    canvas_height: u32,
    bottom_reserve: u32,
}

struct TextOverlayRequest {
    key: TextOverlayRequestKey,
}

struct TextOverlayResult {
    key: TextOverlayRequestKey,
    overlay: Option<CachedTextOverlay>,
}

#[derive(Default)]
struct TextOverlayWorkerData {
    pending: Option<TextOverlayRequest>,
    prefetch: VecDeque<TextOverlayRequest>,
    results: VecDeque<TextOverlayResult>,
    in_flight: Option<TextOverlayRequestKey>,
    shutdown: bool,
}

#[derive(Default)]
struct TextOverlayWorkerState {
    data: Mutex<TextOverlayWorkerData>,
    wake: Condvar,
}

struct TextOverlayWorker {
    font: Option<FontRenderer>,
    fallback_paths: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
pub(crate) struct SubtitleLayout {
    pub(crate) canvas_width: u32,
    pub(crate) canvas_height: u32,
    pub(crate) video_x: u32,
    pub(crate) video_y: u32,
    pub(crate) video_width: u32,
    pub(crate) video_height: u32,
}

impl SubtitleRenderer {
    pub(crate) fn new(fonts: &FontSystem, language: Option<&str>) -> Self {
        let subtitle_fonts = fonts.resolve_all_for_language(FontRole::Subtitle, language);
        let mut fallback_paths = subtitle_fonts;
        fallback_paths.extend(fonts.resolve_all(FontRole::Ui).map(Path::to_path_buf));
        let mut unique_paths = HashSet::new();
        fallback_paths.retain(|path| unique_paths.insert(path.clone()));
        Self::with_font_paths(fallback_paths)
    }

    fn with_font_paths(fallback_paths: Vec<PathBuf>) -> Self {
        let worker = Arc::new(TextOverlayWorkerState::default());
        let worker_thread = Arc::clone(&worker);
        thread::Builder::new()
            .name(String::from("enzo-subtitle-render"))
            .spawn(move || text_overlay_worker(worker_thread, fallback_paths))
            .expect("subtitle rendering worker should start");
        Self {
            worker,
            current_key: None,
            current_generation: 0,
            submitted_generation: None,
            cached_overlay: None,
            ready_overlays: VecDeque::new(),
            prefetch_initialized: false,
            #[cfg(test)]
            request_submissions: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn without_font() -> Self {
        Self::with_font_paths(Vec::new())
    }

    pub(crate) fn render(
        &mut self,
        frame: &mut [u8],
        layout: SubtitleLayout,
        track: &SubtitleTrack,
        position: Duration,
        bottom_reserve: u32,
    ) {
        let width = layout.canvas_width;
        let height = layout.canvas_height;
        if width == 0 || height == 0 || frame.len() < width as usize * height as usize * 3 {
            return;
        }
        for bitmap in track.active_bitmaps(position) {
            draw_bitmap_subtitle(frame, layout, bitmap, bottom_reserve);
        }
        let current_key = track
            .active_lines(position)
            .map(|lines| TextOverlayRequestKey {
                lines,
                canvas_width: width,
                canvas_height: height,
                bottom_reserve,
            });
        let refresh_prefetch =
            self.set_current_text_request(current_key) || !self.prefetch_initialized;
        let prefetch = if refresh_prefetch {
            track
                .upcoming_line_sets(position, MAX_SUBTITLE_PREFETCH_CUES)
                .into_iter()
                .map(|lines| TextOverlayRequestKey {
                    lines,
                    canvas_width: width,
                    canvas_height: height,
                    bottom_reserve,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.prefetch_initialized = true;
        self.poll_and_submit(refresh_prefetch, prefetch);
        if let Some(overlay) = self.cached_overlay.as_ref() {
            composite_text_overlay(frame, overlay);
        }
    }

    fn set_current_text_request(&mut self, key: Option<TextOverlayRequestKey>) -> bool {
        if self.current_key == key {
            return false;
        }
        self.current_key = key;
        self.current_generation = self.current_generation.wrapping_add(1);
        self.submitted_generation = None;
        self.cached_overlay = None;
        true
    }

    pub(crate) fn poll_ready(&mut self) -> bool {
        let worker = Arc::clone(&self.worker);
        let mut data = match worker.data.try_lock() {
            Ok(data) => data,
            Err(TryLockError::WouldBlock) => return false,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
        };
        self.collect_results(&mut data)
    }

    fn poll_and_submit(
        &mut self,
        refresh_prefetch: bool,
        prefetch_keys: Vec<TextOverlayRequestKey>,
    ) {
        let worker = Arc::clone(&self.worker);
        let mut data = match worker.data.try_lock() {
            Ok(data) => data,
            Err(TryLockError::WouldBlock) => return,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
        };
        self.collect_results(&mut data);
        if self.cached_overlay.is_none()
            && self.submitted_generation != Some(self.current_generation)
            && let Some(key) = self.current_key.clone()
        {
            if data.in_flight.as_ref() == Some(&key) {
                self.submitted_generation = Some(self.current_generation);
            } else {
                if let Some(index) = data.prefetch.iter().position(|request| request.key == key) {
                    data.prefetch.remove(index);
                }
                data.pending = Some(TextOverlayRequest { key });
                self.submitted_generation = Some(self.current_generation);
                #[cfg(test)]
                {
                    self.request_submissions += 1;
                }
                self.worker.wake.notify_one();
            }
        }
        if refresh_prefetch {
            data.prefetch.clear();
            for key in prefetch_keys {
                if Some(&key) == self.current_key.as_ref()
                    || data
                        .pending
                        .as_ref()
                        .is_some_and(|request| request.key == key)
                    || data.in_flight.as_ref() == Some(&key)
                    || data.prefetch.iter().any(|request| request.key == key)
                    || data.results.iter().any(|result| result.key == key)
                    || self.ready_overlays.iter().any(|result| result.key == key)
                {
                    continue;
                }
                data.prefetch.push_back(TextOverlayRequest { key });
            }
            self.worker.wake.notify_one();
        }
    }

    fn collect_results(&mut self, data: &mut TextOverlayWorkerData) -> bool {
        let mut current_ready = false;
        while let Some(result) = data.results.pop_front() {
            if Some(&result.key) == self.current_key.as_ref() {
                self.cached_overlay = result.overlay;
                current_ready = true;
            } else {
                self.ready_overlays.push_back(result);
                while self.ready_overlays.len() > MAX_READY_SUBTITLE_OVERLAYS {
                    self.ready_overlays.pop_front();
                }
            }
        }
        if self.cached_overlay.is_none()
            && let Some(key) = self.current_key.as_ref()
            && let Some(index) = self
                .ready_overlays
                .iter()
                .position(|result| &result.key == key)
            && let Some(result) = self.ready_overlays.remove(index)
        {
            self.cached_overlay = result.overlay;
            current_ready = true;
        }
        current_ready
    }
}

impl Drop for SubtitleRenderer {
    fn drop(&mut self) {
        let mut data = match self.worker.data.lock() {
            Ok(data) => data,
            Err(error) => error.into_inner(),
        };
        data.shutdown = true;
        data.pending = None;
        data.prefetch.clear();
        self.worker.wake.notify_one();
    }
}

fn text_overlay_worker(state: Arc<TextOverlayWorkerState>, fallback_paths: Vec<PathBuf>) {
    let font = open_first_font(&fallback_paths, 26);
    let mut renderer = TextOverlayWorker {
        font,
        fallback_paths,
    };
    loop {
        let (request, speculative) = {
            let mut data = match state.data.lock() {
                Ok(data) => data,
                Err(error) => error.into_inner(),
            };
            while data.pending.is_none() && data.prefetch.is_empty() && !data.shutdown {
                data = match state.wake.wait(data) {
                    Ok(data) => data,
                    Err(error) => error.into_inner(),
                };
            }
            if data.shutdown {
                return;
            }
            let (request, speculative) = if let Some(request) = data.pending.take() {
                (request, false)
            } else {
                (
                    data.prefetch
                        .pop_front()
                        .expect("prefetch request should exist"),
                    true,
                )
            };
            data.in_flight = Some(request.key.clone());
            (request, speculative)
        };
        let overlay = renderer.build_overlay(&request.key);
        let mut data = match state.data.lock() {
            Ok(data) => data,
            Err(error) => error.into_inner(),
        };
        if data.shutdown {
            return;
        }
        data.in_flight = None;
        data.results.push_back(TextOverlayResult {
            key: request.key,
            overlay,
        });
        while data.results.len() > MAX_READY_SUBTITLE_OVERLAYS {
            data.results.pop_front();
        }
        drop(data);
        if speculative {
            thread::yield_now();
        }
    }
}

impl TextOverlayWorker {
    fn build_overlay(&mut self, key: &TextOverlayRequestKey) -> Option<CachedTextOverlay> {
        let cached = self.prepare_layout(key);
        if cached.lines.is_empty() {
            return None;
        }
        let line_gap = (cached.line_height / 5).max(2);
        let block_height = cached
            .line_height
            .saturating_mul(cached.lines.len() as u32)
            .saturating_add(line_gap.saturating_mul(cached.lines.len().saturating_sub(1) as u32));
        let bottom_margin = subtitle_bottom_margin(key.canvas_height)
            .max(key.bottom_reserve.saturating_add(8))
            .min(key.canvas_height.saturating_sub(1));
        let start_y = key
            .canvas_height
            .saturating_sub(bottom_margin)
            .saturating_sub(block_height);
        build_text_overlay(
            self.font.as_mut(),
            key.canvas_width,
            key.canvas_height,
            start_y,
            cached.line_height,
            line_gap,
            cached.fallback_scale,
            &cached.lines,
        )
    }

    fn prepare_layout(&mut self, key: &TextOverlayRequestKey) -> CachedSubtitleLayout {
        let font_size = subtitle_font_size(key.canvas_width, key.canvas_height);
        let fallback_scale = fallback_text_scale(key.canvas_width, key.canvas_height);
        let max_width =
            ((f64::from(key.canvas_width) * MAX_SUBTITLE_WIDTH_RATIO).round() as u32).max(1);
        let mut font = if let Some(font) = self.font.as_mut() {
            font.set_pixel_size(font_size).then_some(font)
        } else {
            None
        };
        if let Some(font) = font.as_deref_mut() {
            let text = key.lines.join("\n");
            let mut loaded = font.fallback_count();
            for path in &self.fallback_paths {
                if loaded >= MAX_SUBTITLE_FALLBACK_FONTS || font.covers_text(&text) {
                    break;
                }
                loaded += font.add_fallback_path_for_text(path, &text) as usize;
            }
        }
        let line_height = font
            .as_ref()
            .map(|font| font.line_height())
            .unwrap_or(7 * fallback_scale)
            .max(1);
        let lines = prepare_subtitle_lines(&key.lines, max_width, fallback_scale, font);
        CachedSubtitleLayout {
            fallback_scale,
            lines,
            line_height,
        }
    }
}
