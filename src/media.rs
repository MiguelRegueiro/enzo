use std::{
    collections::VecDeque,
    ffi::{CString, c_char, c_double, c_int, c_uchar},
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

const MAX_PLAYBACK_FPS: f64 = 30.0;
const ERROR_BUFFER_LEN: usize = 4096;
const INFO_TEXT_LEN: usize = 64;
const HDR_PQ: c_int = 1;
const HDR_HLG: c_int = 2;
const AUDIO_OUTPUT_SUMMARY: &str = "PCM S16 · Stereo · 48 kHz";
const DISPLAY_RATE_WINDOW: Duration = Duration::from_secs(2);

#[repr(C)]
struct RigVideoInfo {
    width: u32,
    height: u32,
    fps: c_double,
    duration: c_double,
    has_audio: c_int,
    seekable: c_int,
    codec: [c_char; INFO_TEXT_LEN],
    profile: [c_char; INFO_TEXT_LEN],
    container: [c_char; INFO_TEXT_LEN],
    hdr: c_int,
}

#[repr(C)]
struct RigVideoDecoderOpaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn rig_probe_video(
        path: *const c_char,
        out: *mut RigVideoInfo,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    fn rig_video_decoder_open(
        path: *const c_char,
        out_width: c_int,
        out_height: c_int,
        fps: c_double,
        out: *mut *mut RigVideoDecoderOpaque,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    fn rig_video_decoder_next(
        decoder: *mut RigVideoDecoderOpaque,
        rgb_out: *mut c_uchar,
        pts_out: *mut c_double,
        stop_flag: *const c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    fn rig_video_decoder_seek(
        decoder: *mut RigVideoDecoderOpaque,
        seconds: c_double,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    fn rig_video_decoder_close(decoder: *mut RigVideoDecoderOpaque);
    fn rig_play_audio(
        path: *const c_char,
        audio_stream_index: c_int,
        stop_flag: *const c_int,
        pause_flag: *const c_int,
        mute_flag: *const c_int,
        seek_generation: *const c_int,
        seek_micros: *const i64,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
}

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
    let mut info = RigVideoInfo {
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
        unsafe { rig_probe_video(path.as_ptr(), &mut info, error.as_mut_ptr(), error.len()) };
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

fn fixed_info_text(value: &[c_char; INFO_TEXT_LEN]) -> Option<String> {
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
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a")
        .arg("-show_entries")
        .arg("stream=index,codec_name,channels,channel_layout,sample_rate:stream_tags=language,title:stream_disposition=default")
        .arg("-of")
        .arg("compact=p=0:nk=0")
        .arg(path)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .enumerate()
        .filter_map(|(fallback, line)| parse_audio_track(line, fallback))
        .collect()
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

fn parse_audio_track(line: &str, fallback_index: usize) -> Option<AudioTrack> {
    let mut probe = AudioTrackProbe::default();
    for field in line.split('|') {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        match key {
            "index" => probe.stream_index = value.parse().ok(),
            "codec_name" => probe.codec = non_empty(value),
            "channels" => probe.channels = value.parse().ok(),
            "channel_layout" => probe.channel_layout = non_empty(value),
            "sample_rate" => probe.sample_rate = value.parse().ok(),
            "tag:language" => probe.language = normalize_audio_language(value),
            "tag:title" => probe.title = non_empty(value),
            "disposition:default" => probe.default = value == "1",
            _ => {}
        }
    }

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

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    NewFrame { pts: Duration },
    NoFrame,
    Ended,
}

#[derive(Default)]
struct DisplayRate {
    delivered_at: VecDeque<Instant>,
}

impl DisplayRate {
    fn record(&mut self, now: Instant) {
        self.delivered_at.push_back(now);
        let cutoff = now.checked_sub(DISPLAY_RATE_WINDOW).unwrap_or(now);
        while self
            .delivered_at
            .front()
            .is_some_and(|sample| *sample < cutoff)
        {
            self.delivered_at.pop_front();
        }
    }

    fn measured_at(&self, now: Instant) -> Option<f64> {
        let cutoff = now.checked_sub(DISPLAY_RATE_WINDOW).unwrap_or(now);
        let mut samples = self
            .delivered_at
            .iter()
            .copied()
            .filter(|sample| *sample >= cutoff);
        let first = samples.next()?;
        let mut last = first;
        let mut intervals = 0_u32;
        for sample in samples {
            last = sample;
            intervals = intervals.saturating_add(1);
        }
        let elapsed = last.saturating_duration_since(first).as_secs_f64();
        (intervals > 0 && elapsed > 0.0).then_some(f64::from(intervals) / elapsed)
    }
}

struct NativeVideoDecoder(*mut RigVideoDecoderOpaque);

unsafe impl Send for NativeVideoDecoder {}

impl NativeVideoDecoder {
    fn open(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        let path = path_cstring(path)?;
        let mut decoder = std::ptr::null_mut();
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            rig_video_decoder_open(
                path.as_ptr(),
                width.try_into().context("video width is too large")?,
                height.try_into().context("video height is too large")?,
                fps,
                &mut decoder,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if status < 0 {
            bail!("{}", error.message("failed to open video decoder"));
        }
        if decoder.is_null() {
            bail!("video decoder returned a null handle");
        }
        Ok(Self(decoder))
    }

    fn next_frame(&mut self, frame: &mut [u8], stop: &AtomicI32) -> Result<Option<f64>> {
        let mut pts = 0.0;
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            rig_video_decoder_next(
                self.0,
                frame.as_mut_ptr(),
                &mut pts,
                stop.as_ptr(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match status {
            1 => Ok(Some(pts)),
            0 => Ok(None),
            _ => bail!("{}", error.message("failed to decode video frame")),
        }
    }

    fn seek(&mut self, position: Duration) -> Result<()> {
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            rig_video_decoder_seek(
                self.0,
                position.as_secs_f64(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if status < 0 {
            bail!("{}", error.message("failed to seek video"));
        }
        Ok(())
    }
}

impl Drop for NativeVideoDecoder {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                rig_video_decoder_close(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

#[derive(Default)]
struct LatestFrame {
    frame: Option<Vec<u8>>,
    pts: Duration,
    ended: bool,
    error: Option<String>,
    serial: u64,
}

pub(crate) struct VideoDecoder {
    latest_frame: Arc<Mutex<LatestFrame>>,
    delivered_serial: u64,
    display_rate: DisplayRate,
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    frame_thread: Option<thread::JoinHandle<()>>,
}

impl VideoDecoder {
    pub(crate) fn spawn_at(
        path: &Path,
        width: u32,
        height: u32,
        fps: f64,
        position: Duration,
        paused: bool,
    ) -> Result<Self> {
        let native = NativeVideoDecoder::open(path, width, height, fps)?;
        let frame_len = frame_len(width, height)?;
        let latest_frame = Arc::new(Mutex::new(LatestFrame::default()));
        let frame_target = Arc::clone(&latest_frame);
        let stop = Arc::new(AtomicI32::new(0));
        let stop_thread = Arc::clone(&stop);
        let pause = Arc::new(AtomicI32::new(i32::from(paused)));
        let pause_thread = Arc::clone(&pause);
        let seek_generation = Arc::new(AtomicI32::new(i32::from(
            position > Duration::ZERO || paused,
        )));
        let seek_generation_thread = Arc::clone(&seek_generation);
        let seek_micros = Arc::new(AtomicI64::new(duration_micros_i64(position)));
        let seek_micros_thread = Arc::clone(&seek_micros);

        let frame_thread = thread::spawn(move || {
            run_video_decode_thread(
                native,
                frame_len,
                fps,
                frame_target,
                stop_thread,
                pause_thread,
                seek_generation_thread,
                seek_micros_thread,
            );
        });

        Ok(Self {
            latest_frame,
            delivered_serial: 0,
            display_rate: DisplayRate::default(),
            stop,
            pause,
            seek_generation,
            seek_micros,
            frame_thread: Some(frame_thread),
        })
    }

    pub(crate) fn read_latest_frame(&mut self, frame: &mut [u8]) -> io::Result<FrameStatus> {
        let mut state = self
            .latest_frame
            .lock()
            .map_err(|_| io::Error::other("video frame state is poisoned"))?;
        if state.serial != self.delivered_serial {
            let Some(latest_frame) = state.frame.as_ref() else {
                return Ok(FrameStatus::NoFrame);
            };
            if latest_frame.len() != frame.len() {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "video frame has {} bytes, expected {}",
                        latest_frame.len(),
                        frame.len()
                    ),
                ));
            }
            frame.copy_from_slice(latest_frame);
            self.delivered_serial = state.serial;
            let pts = state.pts;
            drop(state);
            self.display_rate.record(Instant::now());
            Ok(FrameStatus::NewFrame { pts })
        } else if let Some(error) = state.error.take() {
            Err(io::Error::other(error))
        } else if state.ended {
            Ok(FrameStatus::Ended)
        } else {
            Ok(FrameStatus::NoFrame)
        }
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.stop.store(1, Ordering::Relaxed);
        if let Some(handle) = self.frame_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.pause.store(i32::from(paused), Ordering::Relaxed);
    }

    pub(crate) fn display_fps(&self, now: Instant) -> Option<f64> {
        self.display_rate.measured_at(now)
    }

    pub(crate) fn seek(&mut self, position: Duration) {
        self.seek_micros
            .store(duration_micros_i64(position), Ordering::Relaxed);
        self.seek_generation.fetch_add(1, Ordering::Relaxed);
        self.display_rate.delivered_at.clear();
        if let Ok(mut state) = self.latest_frame.lock() {
            state.frame = None;
            state.error = None;
            state.ended = false;
            state.serial = state.serial.wrapping_add(1);
            self.delivered_serial = state.serial;
        }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_video_decode_thread(
    mut native: NativeVideoDecoder,
    frame_len: usize,
    fps: f64,
    latest_frame: Arc<Mutex<LatestFrame>>,
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
) {
    let mut started_at = Instant::now();
    let fallback_interval = 1.0 / fps.max(1.0);
    let mut fallback_pts = 0.0;
    let mut buffer = vec![0_u8; frame_len];
    let mut seen_seek_generation = 0;
    let mut force_next_frame = false;

    loop {
        if stop.load(Ordering::Relaxed) != 0 {
            mark_ended(&latest_frame);
            break;
        }

        if let Some(position) =
            take_seek_request(&seek_generation, &seek_micros, &mut seen_seek_generation)
        {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                position,
                &mut started_at,
                &mut fallback_pts,
            ) {
                mark_error(&latest_frame, error.to_string());
                break;
            }
            force_next_frame = true;
        }

        if !force_next_frame {
            match wait_while_paused(
                &stop,
                &pause,
                &seek_generation,
                &seek_micros,
                &mut seen_seek_generation,
            ) {
                PauseWait::Ready(paused_for) => {
                    started_at += paused_for;
                }
                PauseWait::Seek(position, paused_for) => {
                    started_at += paused_for;
                    if let Err(error) = seek_video_thread(
                        &mut native,
                        &latest_frame,
                        position,
                        &mut started_at,
                        &mut fallback_pts,
                    ) {
                        mark_error(&latest_frame, error.to_string());
                        break;
                    }
                    force_next_frame = true;
                }
                PauseWait::Stopped => {
                    mark_ended(&latest_frame);
                    break;
                }
            }
        }

        let pts = match native.next_frame(&mut buffer, &stop) {
            Ok(Some(pts)) => pts,
            Ok(None) => {
                mark_ended(&latest_frame);
                break;
            }
            Err(error) => {
                mark_error(&latest_frame, error.to_string());
                break;
            }
        };

        let pts = if pts.is_finite() && pts >= 0.0 {
            pts
        } else {
            let pts = fallback_pts;
            fallback_pts += fallback_interval;
            pts
        };
        let pts_duration = Duration::from_secs_f64(pts);
        if !force_next_frame {
            let mut due_at = started_at + pts_duration;
            loop {
                if stop.load(Ordering::Relaxed) != 0 {
                    mark_ended(&latest_frame);
                    return;
                }
                match wait_while_paused(
                    &stop,
                    &pause,
                    &seek_generation,
                    &seek_micros,
                    &mut seen_seek_generation,
                ) {
                    PauseWait::Ready(paused_for) => {
                        started_at += paused_for;
                        due_at += paused_for;
                    }
                    PauseWait::Seek(position, paused_for) => {
                        started_at += paused_for;
                        if let Err(error) = seek_video_thread(
                            &mut native,
                            &latest_frame,
                            position,
                            &mut started_at,
                            &mut fallback_pts,
                        ) {
                            mark_error(&latest_frame, error.to_string());
                            return;
                        }
                        force_next_frame = true;
                        break;
                    }
                    PauseWait::Stopped => {
                        mark_ended(&latest_frame);
                        return;
                    }
                }

                let now = Instant::now();
                if due_at <= now {
                    break;
                }
                thread::sleep((due_at - now).min(Duration::from_millis(10)));
            }
            if force_next_frame {
                continue;
            }
        }

        if let Some(position) =
            take_seek_request(&seek_generation, &seek_micros, &mut seen_seek_generation)
        {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                position,
                &mut started_at,
                &mut fallback_pts,
            ) {
                mark_error(&latest_frame, error.to_string());
                break;
            }
            force_next_frame = true;
            continue;
        }

        buffer = store_latest_frame(
            &latest_frame,
            buffer,
            frame_len,
            pts_duration,
            &seek_generation,
            seen_seek_generation,
        );
        force_next_frame = false;
    }
}

enum PauseWait {
    Ready(Duration),
    Seek(Duration, Duration),
    Stopped,
}

fn wait_while_paused(
    stop: &AtomicI32,
    pause: &AtomicI32,
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seen_seek_generation: &mut i32,
) -> PauseWait {
    if pause.load(Ordering::Relaxed) == 0 {
        return PauseWait::Ready(Duration::ZERO);
    }

    let paused_at = Instant::now();
    while pause.load(Ordering::Relaxed) != 0 {
        if stop.load(Ordering::Relaxed) != 0 {
            return PauseWait::Stopped;
        }
        if let Some(position) =
            take_seek_request(seek_generation, seek_micros, seen_seek_generation)
        {
            return PauseWait::Seek(position, paused_at.elapsed());
        }
        thread::sleep(Duration::from_millis(5));
    }
    PauseWait::Ready(paused_at.elapsed())
}

fn take_seek_request(
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seen_seek_generation: &mut i32,
) -> Option<Duration> {
    let generation = seek_generation.load(Ordering::Relaxed);
    if generation == *seen_seek_generation {
        return None;
    }
    *seen_seek_generation = generation;
    let micros = seek_micros.load(Ordering::Relaxed).max(0) as u64;
    Some(Duration::from_micros(micros))
}

fn seek_video_thread(
    native: &mut NativeVideoDecoder,
    latest_frame: &Arc<Mutex<LatestFrame>>,
    position: Duration,
    started_at: &mut Instant,
    fallback_pts: &mut f64,
) -> Result<()> {
    native.seek(position)?;
    reset_frame_state(latest_frame);
    *started_at = Instant::now() - position;
    *fallback_pts = position.as_secs_f64();
    Ok(())
}

pub(crate) struct AudioPlayer {
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    mute: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    handle: Option<thread::JoinHandle<Result<()>>>,
    finished: bool,
}

impl AudioPlayer {
    pub(crate) fn spawn_at(
        path: &Path,
        audio_stream_index: Option<usize>,
        position: Duration,
        paused: bool,
        muted: bool,
    ) -> Result<Self> {
        let path = path_cstring(path)?;
        let audio_stream_index = audio_stream_index
            .map(i32::try_from)
            .transpose()
            .context("audio stream index is too large")?
            .filter(|index| *index >= 0)
            .unwrap_or(-1);
        let stop = Arc::new(AtomicI32::new(0));
        let stop_thread = Arc::clone(&stop);
        let pause = Arc::new(AtomicI32::new(i32::from(paused)));
        let pause_thread = Arc::clone(&pause);
        let mute = Arc::new(AtomicI32::new(i32::from(muted)));
        let mute_thread = Arc::clone(&mute);
        let seek_generation = Arc::new(AtomicI32::new(i32::from(position > Duration::ZERO)));
        let seek_generation_thread = Arc::clone(&seek_generation);
        let seek_micros = Arc::new(AtomicI64::new(duration_micros_i64(position)));
        let seek_micros_thread = Arc::clone(&seek_micros);
        let handle = thread::spawn(move || {
            let mut error = ErrorBuffer::new();
            let status = unsafe {
                rig_play_audio(
                    path.as_ptr(),
                    audio_stream_index,
                    stop_thread.as_ptr(),
                    pause_thread.as_ptr(),
                    mute_thread.as_ptr(),
                    seek_generation_thread.as_ptr(),
                    seek_micros_thread.as_ptr(),
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            if status < 0 {
                bail!("{}", error.message("audio playback failed"));
            }
            Ok(())
        });

        Ok(Self {
            stop,
            pause,
            mute,
            seek_generation,
            seek_micros,
            handle: Some(handle),
            finished: false,
        })
    }

    pub(crate) fn is_finished(&mut self) -> Result<bool> {
        if self.finished {
            return Ok(true);
        }
        let Some(handle) = self.handle.as_ref() else {
            self.finished = true;
            return Ok(true);
        };
        if !handle.is_finished() {
            return Ok(false);
        }

        let handle = self.handle.take().expect("audio handle should exist");
        self.finished = true;
        handle
            .join()
            .unwrap_or_else(|_| Err(anyhow!("audio playback thread panicked")))?;
        Ok(true)
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.stop.store(1, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            self.finished = true;
            handle
                .join()
                .unwrap_or_else(|_| Err(anyhow!("audio playback thread panicked")))?;
        }
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.pause.store(i32::from(paused), Ordering::Relaxed);
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.mute.store(i32::from(muted), Ordering::Relaxed);
    }

    pub(crate) fn seek(&self, position: Duration) {
        self.seek_micros
            .store(duration_micros_i64(position), Ordering::Relaxed);
        self.seek_generation.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct ErrorBuffer {
    bytes: [c_char; ERROR_BUFFER_LEN],
}

impl ErrorBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; ERROR_BUFFER_LEN],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut c_char {
        self.bytes.as_mut_ptr()
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn message(&self, fallback: &str) -> String {
        let bytes = self
            .bytes
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| byte as u8)
            .collect::<Vec<_>>();
        if bytes.is_empty() {
            fallback.to_string()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }
}

fn path_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains an interior NUL byte: {}", path.display()))
}

fn frame_len(width: u32, height: u32) -> Result<usize> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("frame dimensions are too large"))?;
    pixels
        .checked_mul(3)
        .map(|bytes| bytes as usize)
        .ok_or_else(|| anyhow!("frame buffer is too large"))
}

fn duration_micros_i64(duration: Duration) -> i64 {
    duration.as_micros().min(i64::MAX as u128) as i64
}

fn store_latest_frame(
    state: &Arc<Mutex<LatestFrame>>,
    frame: Vec<u8>,
    frame_len: usize,
    pts: Duration,
    seek_generation: &AtomicI32,
    seen_seek_generation: i32,
) -> Vec<u8> {
    if seek_generation.load(Ordering::Relaxed) != seen_seek_generation {
        return frame;
    }

    let Ok(mut state) = state.lock() else {
        return frame;
    };
    if seek_generation.load(Ordering::Relaxed) != seen_seek_generation {
        return frame;
    }

    let old_frame = state.frame.replace(frame);
    state.pts = pts;
    state.ended = false;
    state.serial = state.serial.wrapping_add(1);
    old_frame.unwrap_or_else(|| vec![0_u8; frame_len])
}

fn reset_frame_state(state: &Arc<Mutex<LatestFrame>>) {
    if let Ok(mut state) = state.lock() {
        state.frame = None;
        state.error = None;
        state.ended = false;
        state.serial = state.serial.wrapping_add(1);
    }
}

fn mark_ended(state: &Arc<Mutex<LatestFrame>>) {
    if let Ok(mut state) = state.lock() {
        state.ended = true;
    }
}

fn mark_error(state: &Arc<Mutex<LatestFrame>>, error: String) {
    if let Ok(mut state) = state.lock() {
        state.error = Some(error);
        state.ended = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_buffer_uses_fallback_when_empty() {
        let error = ErrorBuffer::new();

        assert_eq!(error.message("fallback"), "fallback");
    }

    #[test]
    fn parses_audio_track_labels_from_ffprobe_output() {
        let track = parse_audio_track(
            "index=2|codec_name=aac|channels=6|channel_layout=5.1|sample_rate=48000|tag:language=jpn|tag:title=Japanese 5.1|disposition:default=1",
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
        let track = parse_audio_track("index=7|codec_name=|channels=N/A", 2)
            .expect("audio track should parse");

        assert_eq!(track.stream_index(), 7);
        assert_eq!(track.label(), "Track 3");
        assert_eq!(
            track.playback_summary(),
            "Output: PCM S16 · Stereo · 48 kHz"
        );
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
    fn display_rate_measures_recent_frame_delivery() {
        let start = Instant::now();
        let mut rate = DisplayRate::default();
        rate.record(start);
        rate.record(start + Duration::from_millis(40));

        assert_eq!(
            rate.measured_at(start + Duration::from_millis(40)),
            Some(25.0)
        );
        assert_eq!(
            rate.measured_at(start + DISPLAY_RATE_WINDOW + Duration::from_secs(1)),
            None
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

    #[test]
    fn stale_frame_is_not_published_after_seek_request() {
        let state = Arc::new(Mutex::new(LatestFrame::default()));
        let seek_generation = AtomicI32::new(2);
        let frame = vec![7, 8, 9];

        let buffer = store_latest_frame(
            &state,
            frame,
            3,
            Duration::from_secs(1),
            &seek_generation,
            1,
        );

        assert_eq!(buffer, vec![7, 8, 9]);
        let state = state.lock().expect("frame state should not be poisoned");
        assert!(state.frame.is_none());
        assert_eq!(state.serial, 0);
    }
}
