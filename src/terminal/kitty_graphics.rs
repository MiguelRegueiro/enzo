use std::io::{self, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use super::{env::inside_tmux, layout::ImageArea};

const KITTY_IMAGE_ID: u32 = 0x52_49_47; // "RIG", within the 24-bit foreground-color-safe range.
pub(crate) const KITTY_IMAGE_IDS: [u32; 2] = [KITTY_IMAGE_ID, KITTY_IMAGE_ID + 1];
pub(crate) const KITTY_PLACEMENT_ID: u32 = 1;
const KITTY_RAW_CHUNK_BYTES: usize = 3 * 4096 / 4;

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
    sequence.clear();
    write!(
        sequence,
        "\x1b[{};{}H",
        placement.area.y.saturating_add(1),
        placement.area.x.saturating_add(1)
    )?;

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

    if let Some(previous_image_id) = placement.previous_image_id
        && previous_image_id != placement.image_id
    {
        write!(sequence, "\x1b_Ga=d,d=I,q=2,i={previous_image_id}\x1b\\")?;
    }

    write_kitty_apc_bytes(out, sequence)
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

pub(crate) fn clear_all_kitty_images(out: &mut impl Write) -> io::Result<()> {
    write_kitty_apc_bytes(out, clear_images_sequence().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_frame_sequence_transmits_rgb_at_requested_area() {
        let frame = [0, 0, 0, 255, 255, 255];
        let area = ImageArea {
            x: 1,
            y: 2,
            cols: 3,
            rows: 4,
        };
        let mut out = Vec::new();
        let mut scratch = Vec::new();

        write_kitty_rgb_frame(
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
}
