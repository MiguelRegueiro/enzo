use std::{
    ffi::CString,
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use super::{ffi::enzo_file_fingerprint, native::ErrorBuffer};

pub(crate) fn file_fingerprint_digest(
    path: &Path,
    len: u64,
    chunk_len: u64,
) -> io::Result<[u8; 32]> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "media path contains NUL"))?;
    let mut digest = [0_u8; 32];
    let mut error = ErrorBuffer::new();
    let status = unsafe {
        enzo_file_fingerprint(
            path.as_ptr(),
            len,
            chunk_len,
            digest.as_mut_ptr(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status < 0 {
        return Err(io::Error::other(
            error.message("failed to fingerprint media"),
        ));
    }
    Ok(digest)
}
