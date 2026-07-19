use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{self, Write},
    os::fd::FromRawFd,
    sync::atomic::{AtomicU64, Ordering},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use super::{
    env::{inside_tmux, looks_like_kitty},
    layout::ImageArea,
};

const KITTY_IMAGE_ID: u32 = 0x52_49_47; // "RIG", within the 24-bit foreground-color-safe range.
pub(crate) const KITTY_IMAGE_IDS: [u32; 2] = [KITTY_IMAGE_ID, KITTY_IMAGE_ID + 1];
pub(crate) const KITTY_PLACEMENT_ID: u32 = 1;
const KITTY_RAW_CHUNK_BYTES: usize = 3 * 4096 / 4;
const SHARED_MEMORY_CREATE_ATTEMPTS: usize = 16;
static SHARED_MEMORY_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KittyFramePlacement {
    pub(crate) image_id: u32,
    pub(crate) placement_id: u32,
    pub(crate) z_index: i32,
    pub(crate) previous_image_id: Option<u32>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) area: ImageArea,
}

pub(crate) fn clear_screen_and_images(out: &mut impl Write) -> io::Result<()> {
    write_kitty_apc_bytes(out, clear_images_sequence().as_bytes())?;
    out.write_all(b"\x1b[2J\x1b[H")
}

fn clear_images_sequence() -> &'static str {
    "\x1b_Ga=d,d=A,q=2\x1b\\"
}

pub(crate) fn write_kitty_rgb_frame(
    out: &mut impl Write,
    placement: KittyFramePlacement,
    frame: &[u8],
    sequence: &mut Vec<u8>,
) -> io::Result<()> {
    // Avoid per-frame base64 work when the terminal can read this host's POSIX shm.
    if shared_memory_preferred()
        && let Ok(mut shared_frame) = SharedMemoryFrame::create(frame)
    {
        write_kitty_shared_memory_image(out, placement, shared_frame.name(), sequence)?;
        shared_frame.relinquish();
        return Ok(());
    }

    write_kitty_direct_image(out, placement, frame, sequence)
}

fn shared_memory_preferred() -> bool {
    is_shared_memory_preferred(
        inside_tmux(),
        looks_like_kitty(),
        ["SSH_CONNECTION", "SSH_CLIENT", "MOSH_IP"]
            .iter()
            .any(|name| std::env::var_os(name).is_some()),
    )
}

fn is_shared_memory_preferred(
    inside_tmux: bool,
    looks_like_kitty: bool,
    has_remote_session_marker: bool,
) -> bool {
    !has_remote_session_marker && (inside_tmux || looks_like_kitty)
}

fn write_kitty_direct_image(
    out: &mut impl Write,
    placement: KittyFramePlacement,
    frame: &[u8],
    sequence: &mut Vec<u8>,
) -> io::Result<()> {
    sequence.clear();
    write_kitty_cursor_position(sequence, placement)?;

    let mut offset = 0;
    let mut first = true;
    let mut encoded = [0_u8; 4096];
    while offset < frame.len() {
        let end = (offset + KITTY_RAW_CHUNK_BYTES).min(frame.len());
        let more = end < frame.len();
        let encoded_len = BASE64
            .encode_slice(&frame[offset..end], &mut encoded)
            .map_err(io::Error::other)?;
        if first {
            write!(
                sequence,
                "\x1b_Ga=T,q=2,f=24,s={},v={},i={},p={},c={},r={},C=1,z={},m={};",
                placement.width,
                placement.height,
                placement.image_id,
                placement.placement_id,
                placement.area.cols,
                placement.area.rows,
                placement.z_index,
                if more { 1 } else { 0 },
            )?;
            sequence.extend_from_slice(&encoded[..encoded_len]);
            sequence.extend_from_slice(b"\x1b\\");
            first = false;
        } else {
            write!(sequence, "\x1b_Gm={};", if more { 1 } else { 0 })?;
            sequence.extend_from_slice(&encoded[..encoded_len]);
            sequence.extend_from_slice(b"\x1b\\");
        }
        offset = end;
    }

    write_kitty_previous_image_delete(sequence, placement)?;
    write_kitty_apc_bytes(out, sequence)
}

fn write_kitty_shared_memory_image(
    out: &mut impl Write,
    placement: KittyFramePlacement,
    shared_memory_name: &CStr,
    sequence: &mut Vec<u8>,
) -> io::Result<()> {
    sequence.clear();
    write_kitty_cursor_position(sequence, placement)?;
    write!(
        sequence,
        "\x1b_Ga=T,q=2,f=24,t=s,s={},v={},i={},p={},c={},r={},C=1,z={};",
        placement.width,
        placement.height,
        placement.image_id,
        placement.placement_id,
        placement.area.cols,
        placement.area.rows,
        placement.z_index,
    )?;
    sequence.extend_from_slice(BASE64.encode(shared_memory_name.to_bytes()).as_bytes());
    sequence.extend_from_slice(b"\x1b\\");
    write_kitty_previous_image_delete(sequence, placement)?;
    write_kitty_apc_bytes(out, sequence)
}

fn write_kitty_cursor_position(
    sequence: &mut impl Write,
    placement: KittyFramePlacement,
) -> io::Result<()> {
    write!(
        sequence,
        "\x1b[{};{}H",
        placement.area.y.saturating_add(1),
        placement.area.x.saturating_add(1)
    )
}

fn write_kitty_previous_image_delete(
    sequence: &mut impl Write,
    placement: KittyFramePlacement,
) -> io::Result<()> {
    if let Some(previous_image_id) = placement.previous_image_id
        && previous_image_id != placement.image_id
    {
        write!(sequence, "\x1b_Ga=d,d=I,q=2,i={previous_image_id}\x1b\\")?;
    }

    Ok(())
}

fn write_kitty_apc_bytes(out: &mut impl Write, sequence: &[u8]) -> io::Result<()> {
    if inside_tmux() {
        out.write_all(&wrap_kitty_apcs_for_tmux(sequence))
    } else {
        out.write_all(sequence)
    }
}

fn wrap_kitty_apcs_for_tmux(sequence: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sequence.len() + sequence.len() / 4);
    let mut i = 0;
    while i < sequence.len() {
        if sequence.len() - i >= 3
            && &sequence[i..i + 3] == b"\x1b_G"
            && let Some(relative_end) = sequence[i + 3..].iter().position(|&byte| byte == 0x1b)
            && sequence.get(i + 3 + relative_end + 1) == Some(&b'\\')
        {
            let body_end = i + 3 + relative_end;
            wrap_sequence_for_tmux(&sequence[i..body_end + 2], &mut out);
            i = body_end + 2;
            continue;
        }
        out.push(sequence[i]);
        i += 1;
    }
    out
}

fn wrap_sequence_for_tmux(sequence: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1bPtmux;");
    for &byte in sequence {
        if byte == 0x1b {
            out.extend_from_slice(b"\x1b\x1b");
        } else {
            out.push(byte);
        }
    }
    out.extend_from_slice(b"\x1b\\");
}

struct SharedMemoryFrame {
    name: CString,
    owned: bool,
}

impl SharedMemoryFrame {
    fn create(frame: &[u8]) -> io::Result<Self> {
        for _ in 0..SHARED_MEMORY_CREATE_ATTEMPTS {
            let serial = SHARED_MEMORY_SERIAL.fetch_add(1, Ordering::Relaxed);
            let name = CString::new(format!(
                "/enzo-tty-graphics-protocol-{}-{serial}",
                std::process::id()
            ))
            .expect("shared memory name must not contain NUL bytes");
            let fd = unsafe {
                libc::shm_open(
                    name.as_ptr(),
                    libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }

            let shared_frame = Self { name, owned: true };
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.set_len(frame.len() as u64)?;
            file.write_all(frame)?;
            return Ok(shared_frame);
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "failed to allocate a unique Kitty graphics shared-memory object",
        ))
    }

    fn name(&self) -> &CStr {
        &self.name
    }

    fn relinquish(&mut self) {
        self.owned = false;
    }
}

impl Drop for SharedMemoryFrame {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                libc::shm_unlink(self.name.as_ptr());
            }
        }
    }
}

pub(crate) fn clear_all_kitty_images(out: &mut impl Write) -> io::Result<()> {
    write_kitty_apc_bytes(out, clear_images_sequence().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_direct_frame_sequence_transmits_rgb_at_requested_area() {
        let frame = [0, 0, 0, 255, 255, 255];
        let area = ImageArea {
            x: 1,
            y: 2,
            cols: 3,
            rows: 4,
        };
        let mut out = Vec::new();
        let mut scratch = Vec::new();

        write_kitty_direct_image(
            &mut out,
            KittyFramePlacement {
                image_id: 7,
                placement_id: 9,
                z_index: 11,
                previous_image_id: None,
                width: 2,
                height: 1,
                area,
            },
            &frame,
            &mut scratch,
        )
        .expect("kitty frame should encode");

        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\x1b[3;2H"));
        assert!(text.contains("a=T,q=2,f=24,s=2,v=1,i=7,p=9,c=3,r=4,C=1,z=11,m=0;"));
    }

    #[test]
    fn kitty_shared_memory_sequence_transmits_only_the_object_name() {
        let frame = [0, 1, 2, 3, 4, 5];
        let shared_frame =
            SharedMemoryFrame::create(&frame).expect("shared memory frame should be created");
        let placement = KittyFramePlacement {
            image_id: 7,
            placement_id: 9,
            z_index: 11,
            previous_image_id: Some(6),
            width: 2,
            height: 1,
            area: ImageArea {
                x: 1,
                y: 2,
                cols: 3,
                rows: 4,
            },
        };
        let mut out = Vec::new();
        let mut scratch = Vec::new();

        write_kitty_shared_memory_image(&mut out, placement, shared_frame.name(), &mut scratch)
            .expect("shared memory command should encode");

        let text = String::from_utf8_lossy(&out);
        let encoded_name = BASE64.encode(shared_frame.name().to_bytes());
        assert!(text.contains("a=T,q=2,f=24,t=s,s=2,v=1,i=7,p=9,c=3,r=4,C=1,z=11;"));
        assert!(text.contains(&encoded_name));
        assert!(!out.windows(frame.len()).any(|window| window == frame));
        assert!(text.contains("a=d,d=I,q=2,i=6"));
    }

    #[test]
    fn shared_memory_frame_contains_the_frame_and_cleans_up_while_owned() {
        use std::io::Read;

        let frame = [0, 1, 2, 3, 4, 5];
        let shared_frame =
            SharedMemoryFrame::create(&frame).expect("shared memory frame should be created");
        let name = shared_frame.name().to_owned();
        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC, 0) };
        assert!(fd >= 0, "shared memory frame should be reopenable");
        let mut file = unsafe { File::from_raw_fd(fd) };
        let mut stored_frame = Vec::new();
        file.read_to_end(&mut stored_frame)
            .expect("shared frame should be readable");
        assert_eq!(stored_frame, frame);

        drop(shared_frame);

        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC, 0) };
        assert_eq!(fd, -1, "owned shared memory frame should be unlinked");
    }

    #[test]
    fn shared_memory_is_selected_for_local_kitty_or_tmux_sessions() {
        assert!(is_shared_memory_preferred(false, true, false));
        assert!(is_shared_memory_preferred(true, false, false));
        assert!(is_shared_memory_preferred(true, true, false));
        assert!(!is_shared_memory_preferred(false, false, false));
        assert!(!is_shared_memory_preferred(false, true, true));
        assert!(!is_shared_memory_preferred(true, false, true));
    }
}
