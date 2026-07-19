use std::{
    collections::VecDeque,
    ffi::c_int,
    io::{self, ErrorKind},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, AtomicI64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

use super::{
    audio::AudioPlayer,
    ffi::{
        EnzoVideoDecoderOpaque, enzo_video_decoder_close, enzo_video_decoder_next,
        enzo_video_decoder_open, enzo_video_decoder_seek,
    },
    native::{ErrorBuffer, duration_micros_i64, path_cstring},
};

const DISPLAY_RATE_WINDOW: Duration = Duration::from_secs(2);
const VIDEO_CLOCK_LEAD: Duration = Duration::from_millis(5);
const VIDEO_CLOCK_DROP_LAG: Duration = Duration::from_millis(75);

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

struct NativeVideoDecoder(*mut EnzoVideoDecoderOpaque);

// SAFETY: the opaque handle is uniquely owned by this value. Every operation
// requires `&mut self`, and `Drop` closes the handle, so moving it to the decode
// thread cannot create concurrent access or outlive its native resources.
unsafe impl Send for NativeVideoDecoder {}

enum NativeFrame {
    Frame(f64),
    Ended,
    Interrupted,
}

impl NativeVideoDecoder {
    fn open(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        let path = path_cstring(path)?;
        let mut decoder = std::ptr::null_mut();
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            enzo_video_decoder_open(
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

    fn next_frame(
        &mut self,
        frame: &mut [u8],
        stop: &AtomicI32,
        seek_generation: &AtomicI32,
        expected_seek_generation: i32,
    ) -> Result<NativeFrame> {
        let mut pts = 0.0;
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            enzo_video_decoder_next(
                self.0,
                frame.as_mut_ptr(),
                frame.len(),
                &mut pts,
                stop.as_ptr(),
                seek_generation.as_ptr(),
                expected_seek_generation,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match status {
            2 => Ok(NativeFrame::Interrupted),
            1 => Ok(NativeFrame::Frame(pts)),
            0 => Ok(NativeFrame::Ended),
            _ => bail!("{}", error.message("failed to decode video frame")),
        }
    }

    fn seek(&mut self, position: Duration, exact: bool) -> Result<()> {
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            enzo_video_decoder_seek(
                self.0,
                position.as_secs_f64(),
                c_int::from(exact),
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
                enzo_video_decoder_close(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

#[derive(Default)]
struct LatestFrame {
    frame: Option<Vec<u8>>,
    pts: Duration,
    seek_generation: i32,
    ended: bool,
    error: Option<String>,
    serial: u64,
}

#[derive(Clone)]
struct VideoThreadState {
    latest_frame: Arc<Mutex<LatestFrame>>,
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    seek_exact: Arc<AtomicI32>,
    released_seek_generation: Arc<AtomicI32>,
    master_clock: Arc<Mutex<Option<Arc<AtomicI64>>>>,
}

pub(crate) struct VideoDecoder {
    shared: VideoThreadState,
    delivered_serial: u64,
    display_rate: DisplayRate,
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
        let seek_generation = Arc::new(AtomicI32::new(i32::from(
            position > Duration::ZERO || paused,
        )));
        let initial_seek_generation = seek_generation.load(Ordering::Relaxed);
        let shared = VideoThreadState {
            latest_frame: Arc::new(Mutex::new(LatestFrame::default())),
            stop: Arc::new(AtomicI32::new(0)),
            pause: Arc::new(AtomicI32::new(i32::from(paused))),
            seek_generation,
            seek_micros: Arc::new(AtomicI64::new(duration_micros_i64(position))),
            seek_exact: Arc::new(AtomicI32::new(1)),
            released_seek_generation: Arc::new(AtomicI32::new(initial_seek_generation)),
            master_clock: Arc::new(Mutex::new(None)),
        };
        let thread_state = shared.clone();

        let frame_thread = thread::spawn(move || {
            run_video_decode_thread(native, frame_len, fps, thread_state);
        });

        Ok(Self {
            shared,
            delivered_serial: 0,
            display_rate: DisplayRate::default(),
            frame_thread: Some(frame_thread),
        })
    }

    pub(crate) fn read_latest_frame(&mut self, frame: &mut [u8]) -> io::Result<FrameStatus> {
        let mut state = self
            .shared
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
        self.request_stop();
        self.join()
    }

    pub(crate) fn request_stop(&self) {
        self.shared.stop.store(1, Ordering::Release);
    }

    pub(crate) fn join(&mut self) -> Result<()> {
        if let Some(handle) = self.frame_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.shared
            .pause
            .store(i32::from(paused), Ordering::Relaxed);
    }

    pub(crate) fn display_fps(&self, now: Instant) -> Option<f64> {
        self.display_rate.measured_at(now)
    }

    pub(crate) fn seek(&mut self, position: Duration) -> i32 {
        self.seek_with_exactness(position, true)
    }

    pub(crate) fn preview_seek(&mut self, position: Duration) -> i32 {
        self.seek_with_exactness(position, false)
    }

    fn seek_with_exactness(&mut self, position: Duration, exact: bool) -> i32 {
        self.shared.pause.store(1, Ordering::Release);
        self.shared
            .seek_exact
            .store(i32::from(exact), Ordering::Release);
        self.shared
            .seek_micros
            .store(duration_micros_i64(position), Ordering::Release);
        let generation = self
            .shared
            .seek_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.display_rate.delivered_at.clear();
        if let Ok(mut state) = self.shared.latest_frame.lock() {
            state.frame = None;
            state.error = None;
            state.ended = false;
            state.serial = state.serial.wrapping_add(1);
            self.delivered_serial = state.serial;
        }
        generation
    }

    pub(crate) fn seek_frame(&self, generation: i32) -> Option<Duration> {
        let state = self.shared.latest_frame.lock().ok()?;
        (state.frame.is_some() && state.seek_generation == generation).then_some(state.pts)
    }

    pub(crate) fn seek_generation(&self) -> i32 {
        self.shared.seek_generation.load(Ordering::Acquire)
    }

    pub(crate) fn release_seek(&self, generation: i32, paused: bool) {
        self.shared
            .released_seek_generation
            .store(generation, Ordering::Release);
        self.shared
            .pause
            .store(i32::from(paused), Ordering::Release);
    }

    pub(crate) fn set_audio_clock(&self, audio: Option<&AudioPlayer>) {
        if let Ok(mut master) = self.shared.master_clock.lock() {
            *master = audio.map(AudioPlayer::playback_clock);
        }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run_video_decode_thread(
    mut native: NativeVideoDecoder,
    frame_len: usize,
    fps: f64,
    shared: VideoThreadState,
) {
    let VideoThreadState {
        latest_frame,
        stop,
        pause,
        seek_generation,
        seek_micros,
        seek_exact,
        released_seek_generation,
        master_clock,
    } = shared;
    let mut started_at = Instant::now();
    let fallback_interval = 1.0 / fps.max(1.0);
    let mut fallback_pts = 0.0;
    let mut buffer = vec![0_u8; frame_len];
    let mut seen_seek_generation = 0;
    let mut force_next_frame = false;
    let mut clocked_seek_generation = 0;
    let mut last_published_pts = None::<Duration>;

    loop {
        if stop.load(Ordering::Relaxed) != 0 {
            mark_ended(&latest_frame);
            break;
        }

        if let Some(request) = take_seek_request(
            &seek_generation,
            &seek_micros,
            &seek_exact,
            &mut seen_seek_generation,
        ) {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                request.position,
                request.exact,
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
                &seek_exact,
                &mut seen_seek_generation,
            ) {
                PauseWait::Ready(paused_for) => {
                    if released_seek_generation.load(Ordering::Acquire) == seen_seek_generation
                        && clocked_seek_generation != seen_seek_generation
                        && let Some(pts) = last_published_pts
                    {
                        started_at = Instant::now() - pts;
                        clocked_seek_generation = seen_seek_generation;
                    } else {
                        started_at += paused_for;
                    }
                }
                PauseWait::Seek(request, paused_for) => {
                    started_at += paused_for;
                    if let Err(error) = seek_video_thread(
                        &mut native,
                        &latest_frame,
                        request.position,
                        request.exact,
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

        let pts =
            match native.next_frame(&mut buffer, &stop, &seek_generation, seen_seek_generation) {
                Ok(NativeFrame::Frame(pts)) => pts,
                Ok(NativeFrame::Interrupted) => continue,
                Ok(NativeFrame::Ended) => {
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
            let mut drop_frame = false;
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
                    &seek_exact,
                    &mut seen_seek_generation,
                ) {
                    PauseWait::Ready(paused_for) => {
                        started_at += paused_for;
                        due_at += paused_for;
                    }
                    PauseWait::Seek(request, paused_for) => {
                        started_at += paused_for;
                        if let Err(error) = seek_video_thread(
                            &mut native,
                            &latest_frame,
                            request.position,
                            request.exact,
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

                if let Some(master_position) = master_clock_position(&master_clock) {
                    if pts_duration.saturating_add(VIDEO_CLOCK_DROP_LAG) < master_position {
                        drop_frame = true;
                        break;
                    }
                    if pts_duration <= master_position.saturating_add(VIDEO_CLOCK_LEAD) {
                        break;
                    }
                    let wait = pts_duration
                        .saturating_sub(master_position)
                        .saturating_sub(VIDEO_CLOCK_LEAD);
                    thread::sleep(wait.min(Duration::from_millis(10)));
                    continue;
                }

                let now = Instant::now();
                if due_at <= now {
                    break;
                }
                thread::sleep((due_at - now).min(Duration::from_millis(10)));
            }
            if force_next_frame || drop_frame {
                continue;
            }
        }

        if let Some(request) = take_seek_request(
            &seek_generation,
            &seek_micros,
            &seek_exact,
            &mut seen_seek_generation,
        ) {
            if let Err(error) = seek_video_thread(
                &mut native,
                &latest_frame,
                request.position,
                request.exact,
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
        last_published_pts = Some(pts_duration);
        force_next_frame = false;
    }
}

fn master_clock_position(master_clock: &Mutex<Option<Arc<AtomicI64>>>) -> Option<Duration> {
    let clock = master_clock.lock().ok()?.clone()?;
    let micros = clock.load(Ordering::Acquire);
    (micros >= 0).then(|| Duration::from_micros(micros as u64))
}

struct SeekRequest {
    position: Duration,
    exact: bool,
}

enum PauseWait {
    Ready(Duration),
    Seek(SeekRequest, Duration),
    Stopped,
}

fn wait_while_paused(
    stop: &AtomicI32,
    pause: &AtomicI32,
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seek_exact: &AtomicI32,
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
        if let Some(request) = take_seek_request(
            seek_generation,
            seek_micros,
            seek_exact,
            seen_seek_generation,
        ) {
            return PauseWait::Seek(request, paused_at.elapsed());
        }
        thread::sleep(Duration::from_millis(5));
    }
    PauseWait::Ready(paused_at.elapsed())
}

fn take_seek_request(
    seek_generation: &AtomicI32,
    seek_micros: &AtomicI64,
    seek_exact: &AtomicI32,
    seen_seek_generation: &mut i32,
) -> Option<SeekRequest> {
    let generation = seek_generation.load(Ordering::Acquire);
    if generation == *seen_seek_generation {
        return None;
    }
    *seen_seek_generation = generation;
    let micros = seek_micros.load(Ordering::Relaxed).max(0) as u64;
    Some(SeekRequest {
        position: Duration::from_micros(micros),
        exact: seek_exact.load(Ordering::Acquire) != 0,
    })
}

fn seek_video_thread(
    native: &mut NativeVideoDecoder,
    latest_frame: &Arc<Mutex<LatestFrame>>,
    position: Duration,
    exact: bool,
    started_at: &mut Instant,
    fallback_pts: &mut f64,
) -> Result<()> {
    native.seek(position, exact)?;
    reset_frame_state(latest_frame);
    *started_at = Instant::now() - position;
    *fallback_pts = position.as_secs_f64();
    Ok(())
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
    state.seek_generation = seen_seek_generation;
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
    use std::process::Command;

    use super::*;

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
    fn native_decoder_rejects_an_undersized_frame_buffer() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media =
            std::env::temp_dir().join(format!("enzo-frame-bounds-test-{}.mkv", std::process::id()));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("color=size=16x16:duration=0.1:rate=1")
            .args(["-c:v", "ffv1"])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let mut decoder =
            NativeVideoDecoder::open(&media, 16, 16, 1.0).expect("video decoder should open");
        let mut short_frame = vec![0_u8; 16 * 16 * 3 - 1];
        let error = decoder
            .next_frame(&mut short_frame, &AtomicI32::new(0), &AtomicI32::new(0), 0)
            .err()
            .expect("undersized output should be rejected");

        assert!(
            error
                .to_string()
                .contains("video frame buffer is too small")
        );
        drop(decoder);
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

    #[test]
    fn rapid_video_seeks_publish_only_the_latest_generation() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media = std::env::temp_dir().join(format!(
            "enzo-rapid-video-seek-test-{}.mkv",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x180:duration=8:rate=30")
            .args(["-c:v", "mpeg4", "-g", "240"])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let mut decoder = VideoDecoder::spawn_at(&media, 64, 36, 30.0, Duration::ZERO, true)
            .expect("video decoder should start");
        let superseded = decoder.seek(Duration::from_millis(7_500));
        thread::sleep(Duration::from_millis(2));
        let latest = decoder.seek(Duration::from_millis(1_000));
        let deadline = Instant::now() + Duration::from_secs(3);
        let latest_pts = loop {
            if let Some(pts) = decoder.seek_frame(latest) {
                break pts;
            }
            assert!(
                Instant::now() < deadline,
                "latest seek frame should become ready"
            );
            thread::sleep(Duration::from_millis(2));
        };

        assert!(decoder.seek_frame(superseded).is_none());
        assert!(latest_pts >= Duration::from_millis(950));
        assert!(latest_pts < Duration::from_millis(1_100));
        decoder.stop().expect("video decoder should stop");
        let _ = std::fs::remove_file(media);
    }

    #[test]
    fn preview_video_seek_publishes_keyframe_without_exact_catchup() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media = std::env::temp_dir().join(format!(
            "enzo-preview-video-seek-test-{}.mkv",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x180:duration=8:rate=30")
            .args(["-c:v", "mpeg4", "-g", "240"])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let mut decoder = VideoDecoder::spawn_at(&media, 64, 36, 30.0, Duration::ZERO, true)
            .expect("video decoder should start");
        let generation = decoder.preview_seek(Duration::from_millis(7_500));
        let deadline = Instant::now() + Duration::from_secs(3);
        let pts = loop {
            if let Some(pts) = decoder.seek_frame(generation) {
                break pts;
            }
            assert!(
                Instant::now() < deadline,
                "preview seek frame should become ready"
            );
            thread::sleep(Duration::from_millis(2));
        };

        assert!(pts < Duration::from_millis(7_000));
        decoder.stop().expect("video decoder should stop");
        let _ = std::fs::remove_file(media);
    }

    #[test]
    fn video_seek_normalizes_nonzero_stream_start_time() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let media = std::env::temp_dir().join(format!(
            "enzo-video-start-time-test-{}.ts",
            std::process::id()
        ));
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=64x64:duration=5:rate=30")
            .args(["-c:v", "mpeg2video", "-g", "30", "-f", "mpegts"])
            .arg(&media)
            .status()
            .expect("ffmpeg should run");
        if !status.success() {
            return;
        }

        let mut decoder = VideoDecoder::spawn_at(&media, 64, 64, 30.0, Duration::ZERO, true)
            .expect("video decoder should start");
        let generation = decoder.seek(Duration::from_millis(2_400));
        let deadline = Instant::now() + Duration::from_secs(3);
        let pts = loop {
            if let Some(pts) = decoder.seek_frame(generation) {
                break pts;
            }
            assert!(
                Instant::now() < deadline,
                "normalized seek frame should become ready"
            );
            thread::sleep(Duration::from_millis(2));
        };

        assert!(pts >= Duration::from_millis(2_350));
        assert!(pts < Duration::from_millis(3_100));
        decoder.stop().expect("video decoder should stop");
        let _ = std::fs::remove_file(media);
    }
}
