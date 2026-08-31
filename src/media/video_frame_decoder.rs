use std::{ffi::c_int, path::Path, sync::atomic::AtomicI32, time::Duration};

use anyhow::{Context, Result, bail};

use super::{
    ffi_support::{ErrorBuffer, path_cstring},
    media_ffi::{
        EnzoVideoDecoderOpaque, enzo_video_decoder_close, enzo_video_decoder_next,
        enzo_video_decoder_open, enzo_video_decoder_seek,
    },
};

pub(super) struct NativeVideoDecoder(*mut EnzoVideoDecoderOpaque);

// SAFETY: the opaque handle is uniquely owned by this value. Every operation
// requires `&mut self`, and `Drop` closes the handle, so moving it to the decode
// thread cannot create concurrent access or outlive its native resources.
unsafe impl Send for NativeVideoDecoder {}

pub(super) enum NativeFrame {
    Frame(f64),
    Ended,
    Interrupted,
    Dropped,
}

impl NativeVideoDecoder {
    pub(super) fn open(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
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

    pub(super) fn next_frame(
        &mut self,
        frame: &mut [u8],
        drop_before_pts: f64,
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
                drop_before_pts,
                stop.as_ptr(),
                seek_generation.as_ptr(),
                expected_seek_generation,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match status {
            3 => Ok(NativeFrame::Dropped),
            2 => Ok(NativeFrame::Interrupted),
            1 => Ok(NativeFrame::Frame(pts)),
            0 => Ok(NativeFrame::Ended),
            _ => bail!("{}", error.message("failed to decode video frame")),
        }
    }

    pub(super) fn seek(&mut self, position: Duration, exact: bool, stop: &AtomicI32) -> Result<()> {
        let mut error = ErrorBuffer::new();
        let status = unsafe {
            enzo_video_decoder_seek(
                self.0,
                position.as_secs_f64(),
                c_int::from(exact),
                stop.as_ptr(),
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

#[cfg(test)]
#[path = "tests/video_frame_decoder.rs"]
mod tests;
