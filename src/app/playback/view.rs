use std::{
    io::{self, Write},
    time::Duration,
};

use crate::{
    font_system::FontSystem,
    overlay::{OverlayState, PlaybackOverlay},
    subtitle::{SubtitleRenderer, SubtitleTrack},
    terminal::{
        KITTY_IMAGE_IDS, KITTY_PLACEMENT_ID, KittyFramePlacement, clear_screen_and_images,
        write_kitty_rgb_frame,
    },
};

use super::layout::{CanvasFrame, TargetFrame};

pub(super) struct PlaybackView<W: Write> {
    pub(super) output: W,
    pub(super) target: TargetFrame,
    pub(super) canvas: CanvasFrame,
    pub(super) sequence: Vec<u8>,
    pub(super) overlay: PlaybackOverlay,
    pub(super) subtitle_renderer: SubtitleRenderer,
    pub(super) frame: Vec<u8>,
    pub(super) composited_frame: Vec<u8>,
    pub(super) previous_image_id: Option<u32>,
    pub(super) frame_serial: u32,
    pub(super) have_frame: bool,
    pub(super) dirty: bool,
    pub(super) last_overlay_visible: bool,
    pub(super) last_status_visible: bool,
    pub(super) last_media_info_visible: bool,
    pub(super) last_media_info_fps_visible: bool,
}

impl<W: Write> PlaybackView<W> {
    pub(super) fn new(
        mut output: W,
        target: TargetFrame,
        canvas: CanvasFrame,
        fonts: &FontSystem,
        subtitle_language: Option<&str>,
    ) -> io::Result<Self> {
        clear_screen_and_images(&mut output)?;
        Ok(Self {
            output,
            target,
            canvas,
            sequence: Vec::with_capacity(canvas.frame_len() + canvas.frame_len() / 2 + 4096),
            overlay: PlaybackOverlay::new(fonts),
            subtitle_renderer: SubtitleRenderer::new(fonts, subtitle_language),
            frame: vec![0_u8; target.frame_len()],
            composited_frame: vec![0_u8; canvas.frame_len()],
            previous_image_id: None,
            frame_serial: 0,
            have_frame: false,
            dirty: false,
            last_overlay_visible: false,
            last_status_visible: false,
            last_media_info_visible: false,
            last_media_info_fps_visible: false,
        })
    }

    pub(super) fn reset_presented_frame(&mut self) {
        self.previous_image_id = None;
        self.have_frame = false;
        self.dirty = false;
        self.last_overlay_visible = false;
        self.last_status_visible = false;
        self.last_media_info_visible = false;
        self.last_media_info_fps_visible = false;
    }

    pub(super) fn reset_overlay_cache(&mut self) {
        self.previous_image_id = None;
        self.last_overlay_visible = false;
        self.last_status_visible = false;
        self.last_media_info_visible = false;
        self.last_media_info_fps_visible = false;
    }

    pub(super) fn render(
        &mut self,
        subtitle_track: Option<&SubtitleTrack>,
        subtitles_visible: bool,
        playback_position: Duration,
        overlay_state: &OverlayState,
    ) -> io::Result<()> {
        if self.frame.len() != self.target.frame_len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "decoded frame length does not match target frame length",
            ));
        }
        if self.composited_frame.len() != self.canvas.frame_len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "composited frame length does not match canvas frame length",
            ));
        }

        self.composited_frame.fill(0);
        copy_video_into_canvas(
            &self.frame,
            self.target,
            &mut self.composited_frame,
            self.canvas,
        );
        if subtitles_visible && let Some(subtitle_track) = subtitle_track {
            self.subtitle_renderer.render(
                &mut self.composited_frame,
                self.canvas.width,
                self.canvas.height,
                subtitle_track,
                playback_position,
                subtitle_bottom_reserve(self.canvas.height, overlay_state.visible),
            );
        }
        self.overlay.render(
            &mut self.composited_frame,
            self.canvas.width,
            self.canvas.height,
            self.canvas.area.rows,
            self.canvas.overlay_scale_percent,
            overlay_state.clone(),
        );

        let image_id = KITTY_IMAGE_IDS[(self.frame_serial as usize) % KITTY_IMAGE_IDS.len()];
        write_kitty_rgb_frame(
            &mut self.output,
            KittyFramePlacement {
                image_id,
                placement_id: KITTY_PLACEMENT_ID,
                z_index: 0,
                previous_image_id: self.previous_image_id,
                width: self.canvas.width,
                height: self.canvas.height,
                area: self.canvas.area,
            },
            &self.composited_frame,
            &mut self.sequence,
        )?;
        self.previous_image_id = Some(image_id);
        self.frame_serial = self.frame_serial.wrapping_add(1);
        self.have_frame = true;
        self.last_overlay_visible = overlay_state.visible;
        self.last_status_visible = overlay_state.status_message.is_some();
        self.last_media_info_visible = overlay_state.media_info.is_some();
        self.last_media_info_fps_visible = overlay_state
            .media_info
            .as_ref()
            .is_some_and(|info| info.display_fps.is_some());
        self.dirty = false;
        Ok(())
    }
}

pub(super) fn copy_video_into_canvas(
    frame: &[u8],
    target: TargetFrame,
    canvas_frame: &mut [u8],
    canvas: CanvasFrame,
) {
    let dst_width = canvas.width as usize;
    let dst_x = canvas.video_x as usize;
    let dst_y = canvas.video_y as usize;
    let video_width = canvas
        .video_width
        .min(canvas.width.saturating_sub(canvas.video_x)) as usize;
    let video_height = canvas
        .video_height
        .min(canvas.height.saturating_sub(canvas.video_y)) as usize;
    if video_width == 0 || video_height == 0 {
        return;
    }

    let src_width = target.width as usize;
    let src_height = target.height as usize;
    let src_row_bytes = src_width * 3;
    if video_width == src_width && video_height == src_height {
        for row in 0..src_height {
            let src_start = row * src_row_bytes;
            let dst_start = ((dst_y + row) * dst_width + dst_x) * 3;
            canvas_frame[dst_start..dst_start + src_row_bytes]
                .copy_from_slice(&frame[src_start..src_start + src_row_bytes]);
        }
        return;
    }

    for dst_row in 0..video_height {
        let src_row = (dst_row * src_height / video_height).min(src_height.saturating_sub(1));
        let src_row_start = src_row * src_row_bytes;
        let dst_row_start = ((dst_y + dst_row) * dst_width + dst_x) * 3;
        for dst_col in 0..video_width {
            let src_col = (dst_col * src_width / video_width).min(src_width.saturating_sub(1));
            let src_offset = src_row_start + src_col * 3;
            let dst_offset = dst_row_start + dst_col * 3;
            canvas_frame[dst_offset..dst_offset + 3]
                .copy_from_slice(&frame[src_offset..src_offset + 3]);
        }
    }
}

fn subtitle_bottom_reserve(height: u32, overlay_visible: bool) -> u32 {
    if overlay_visible {
        (height / 7).clamp(28, 64)
    } else {
        0
    }
}
