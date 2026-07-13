use std::{
    ffi::{CString, c_char, c_double, c_int, c_uchar},
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
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

#[repr(C)]
struct RigVideoInfo {
    width: u32,
    height: u32,
    fps: c_double,
    duration: c_double,
    has_audio: c_int,
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
        stop_flag: *const c_int,
        pause_flag: *const c_int,
        seek_generation: *const c_int,
        seek_micros: *const i64,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: f64,
    pub(crate) duration: Option<Duration>,
    pub(crate) has_audio: bool,
}

pub(crate) fn probe_video(path: &Path) -> Result<VideoInfo> {
    let path = path_cstring(path)?;
    let mut info = RigVideoInfo {
        width: 0,
        height: 0,
        fps: 0.0,
        duration: 0.0,
        has_audio: 0,
    };
    let mut error = ErrorBuffer::new();

    let status =
        unsafe { rig_probe_video(path.as_ptr(), &mut info, error.as_mut_ptr(), error.len()) };
    if status < 0 {
        bail!("{}", error.message("failed to inspect video"));
    }

    Ok(VideoInfo {
        width: info.width,
        height: info.height,
        fps: info
            .fps
            .is_finite()
            .then_some(info.fps)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0)
            .min(MAX_PLAYBACK_FPS),
        duration: info
            .duration
            .is_finite()
            .then_some(info.duration)
            .filter(|duration| *duration > 0.0)
            .map(Duration::from_secs_f64),
        has_audio: info.has_audio != 0,
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    NewFrame { pts: Duration },
    NoFrame,
    Ended,
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
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    frame_thread: Option<thread::JoinHandle<()>>,
}

impl VideoDecoder {
    pub(crate) fn spawn(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        Self::spawn_at(path, width, height, fps, Duration::ZERO, false)
    }

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
            Ok(FrameStatus::NewFrame { pts: state.pts })
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

    pub(crate) fn seek(&mut self, position: Duration) {
        self.seek_micros
            .store(duration_micros_i64(position), Ordering::Relaxed);
        self.seek_generation.fetch_add(1, Ordering::Relaxed);
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
    seek_generation: Arc<AtomicI32>,
    seek_micros: Arc<AtomicI64>,
    handle: Option<thread::JoinHandle<Result<()>>>,
    finished: bool,
}

impl AudioPlayer {
    pub(crate) fn spawn(path: &Path) -> Result<Self> {
        Self::spawn_at(path, Duration::ZERO, false)
    }

    pub(crate) fn spawn_at(path: &Path, position: Duration, paused: bool) -> Result<Self> {
        let path = path_cstring(path)?;
        let stop = Arc::new(AtomicI32::new(0));
        let stop_thread = Arc::clone(&stop);
        let pause = Arc::new(AtomicI32::new(i32::from(paused)));
        let pause_thread = Arc::clone(&pause);
        let seek_generation = Arc::new(AtomicI32::new(i32::from(position > Duration::ZERO)));
        let seek_generation_thread = Arc::clone(&seek_generation);
        let seek_micros = Arc::new(AtomicI64::new(duration_micros_i64(position)));
        let seek_micros_thread = Arc::clone(&seek_micros);
        let handle = thread::spawn(move || {
            let mut error = ErrorBuffer::new();
            let status = unsafe {
                rig_play_audio(
                    path.as_ptr(),
                    stop_thread.as_ptr(),
                    pause_thread.as_ptr(),
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
