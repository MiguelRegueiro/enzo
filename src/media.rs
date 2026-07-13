use std::{
    ffi::{CString, c_char, c_double, c_int, c_uchar},
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
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
    fn rig_video_decoder_close(decoder: *mut RigVideoDecoderOpaque);
    fn rig_play_audio(
        path: *const c_char,
        stop_flag: *const c_int,
        pause_flag: *const c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: f64,
    pub(crate) has_audio: bool,
}

pub(crate) fn probe_video(path: &Path) -> Result<VideoInfo> {
    let path = path_cstring(path)?;
    let mut info = RigVideoInfo {
        width: 0,
        height: 0,
        fps: 0.0,
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
        has_audio: info.has_audio != 0,
    })
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    NewFrame,
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
    ended: bool,
    error: Option<String>,
    serial: u64,
}

pub(crate) struct VideoDecoder {
    latest_frame: Arc<Mutex<LatestFrame>>,
    delivered_serial: u64,
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    frame_thread: Option<thread::JoinHandle<()>>,
}

impl VideoDecoder {
    pub(crate) fn spawn(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        let native = NativeVideoDecoder::open(path, width, height, fps)?;
        let frame_len = frame_len(width, height)?;
        let latest_frame = Arc::new(Mutex::new(LatestFrame::default()));
        let frame_target = Arc::clone(&latest_frame);
        let stop = Arc::new(AtomicI32::new(0));
        let stop_thread = Arc::clone(&stop);
        let pause = Arc::new(AtomicI32::new(0));
        let pause_thread = Arc::clone(&pause);

        let frame_thread = thread::spawn(move || {
            run_video_decode_thread(
                native,
                frame_len,
                fps,
                frame_target,
                stop_thread,
                pause_thread,
            );
        });

        Ok(Self {
            latest_frame,
            delivered_serial: 0,
            stop,
            pause,
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
            Ok(FrameStatus::NewFrame)
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
) {
    let mut started_at = Instant::now();
    let fallback_interval = 1.0 / fps.max(1.0);
    let mut fallback_pts = 0.0;
    let mut buffer = vec![0_u8; frame_len];

    loop {
        if stop.load(Ordering::Relaxed) != 0 {
            mark_ended(&latest_frame);
            break;
        }
        let Some(paused_for) = wait_while_paused(&stop, &pause) else {
            mark_ended(&latest_frame);
            break;
        };
        started_at += paused_for;

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
        let mut due_at = started_at + Duration::from_secs_f64(pts);
        loop {
            if stop.load(Ordering::Relaxed) != 0 {
                mark_ended(&latest_frame);
                return;
            }
            let Some(paused_for) = wait_while_paused(&stop, &pause) else {
                mark_ended(&latest_frame);
                return;
            };
            started_at += paused_for;
            due_at += paused_for;

            let now = Instant::now();
            if due_at <= now {
                break;
            }
            thread::sleep((due_at - now).min(Duration::from_millis(10)));
        }

        buffer = store_latest_frame(&latest_frame, buffer, frame_len);
    }
}

fn wait_while_paused(stop: &AtomicI32, pause: &AtomicI32) -> Option<Duration> {
    if pause.load(Ordering::Relaxed) == 0 {
        return Some(Duration::ZERO);
    }

    let paused_at = Instant::now();
    while pause.load(Ordering::Relaxed) != 0 {
        if stop.load(Ordering::Relaxed) != 0 {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
    Some(paused_at.elapsed())
}

pub(crate) struct AudioPlayer {
    stop: Arc<AtomicI32>,
    pause: Arc<AtomicI32>,
    handle: Option<thread::JoinHandle<Result<()>>>,
    finished: bool,
}

impl AudioPlayer {
    pub(crate) fn spawn(path: &Path) -> Result<Self> {
        let path = path_cstring(path)?;
        let stop = Arc::new(AtomicI32::new(0));
        let stop_thread = Arc::clone(&stop);
        let pause = Arc::new(AtomicI32::new(0));
        let pause_thread = Arc::clone(&pause);
        let handle = thread::spawn(move || {
            let mut error = ErrorBuffer::new();
            let status = unsafe {
                rig_play_audio(
                    path.as_ptr(),
                    stop_thread.as_ptr(),
                    pause_thread.as_ptr(),
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

fn store_latest_frame(
    state: &Arc<Mutex<LatestFrame>>,
    frame: Vec<u8>,
    frame_len: usize,
) -> Vec<u8> {
    let old_frame = if let Ok(mut state) = state.lock() {
        let old_frame = state.frame.replace(frame);
        state.serial = state.serial.wrapping_add(1);
        old_frame
    } else {
        None
    };
    old_frame.unwrap_or_else(|| vec![0_u8; frame_len])
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
}
