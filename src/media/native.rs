use std::{
    ffi::{CString, c_char},
    os::unix::ffi::OsStrExt,
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result};

const ERROR_BUFFER_LEN: usize = 4096;

pub(super) struct ErrorBuffer {
    bytes: [c_char; ERROR_BUFFER_LEN],
}

impl ErrorBuffer {
    pub(super) fn new() -> Self {
        Self {
            bytes: [0; ERROR_BUFFER_LEN],
        }
    }

    pub(super) fn as_mut_ptr(&mut self) -> *mut c_char {
        self.bytes.as_mut_ptr()
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn message(&self, fallback: &str) -> String {
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

pub(super) fn path_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains an interior NUL byte: {}", path.display()))
}

pub(super) fn duration_micros_i64(duration: Duration) -> i64 {
    duration.as_micros().min(i64::MAX as u128) as i64
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
