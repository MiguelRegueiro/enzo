use crate::terminal::{ImageArea, terminal_pixel_size};

const MAX_DECODE_WIDTH: u32 = 1920;
const MAX_DECODE_HEIGHT: u32 = 1080;
const MAX_CANVAS_WIDTH: u32 = 1920;
const MAX_CANVAS_HEIGHT: u32 = 1200;
const NORMAL_OVERLAY_SCALE_PERCENT: u32 = 100;
const MAX_OVERLAY_SCALE_PERCENT: u32 = 125;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TargetFrame {
    pub(super) width: u32,
    pub(super) height: u32,
}

impl TargetFrame {
    pub(super) fn frame_len(self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanvasFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) terminal_width: u32,
    pub(super) terminal_height: u32,
    pub(super) video_x: u32,
    pub(super) video_y: u32,
    pub(super) video_width: u32,
    pub(super) video_height: u32,
    pub(super) overlay_scale_percent: u32,
    pub(super) area: ImageArea,
}

impl CanvasFrame {
    pub(super) fn frame_len(self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

pub(super) fn terminal_target_and_canvas(
    source_width: u32,
    source_height: u32,
) -> (TargetFrame, CanvasFrame) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let cols = cols.max(1);
    let rows = rows.max(1);
    let (pixel_width, pixel_height) = terminal_pixel_size(cols, rows);
    let target = target_for_bounds(source_width, source_height, pixel_width, pixel_height);
    let canvas = canvas_for_terminal(
        source_width,
        source_height,
        cols,
        rows,
        pixel_width,
        pixel_height,
    );
    (target, canvas)
}

fn canvas_for_terminal(
    source_width: u32,
    source_height: u32,
    cols: u16,
    rows: u16,
    pixel_width: u32,
    pixel_height: u32,
) -> CanvasFrame {
    let canvas = cap_pixels(
        pixel_width.max(1),
        pixel_height.max(1),
        MAX_CANVAS_WIDTH,
        MAX_CANVAS_HEIGHT,
    );
    let video = fit_pixels(source_width, source_height, canvas.width, canvas.height);
    let video_x = canvas.width.saturating_sub(video.width) / 2;
    let video_y = canvas.height.saturating_sub(video.height) / 2;
    let overlay_scale_percent =
        overlay_scale_percent(pixel_width, pixel_height, canvas.width, canvas.height);

    CanvasFrame {
        width: canvas.width,
        height: canvas.height,
        terminal_width: pixel_width.max(1),
        terminal_height: pixel_height.max(1),
        video_x,
        video_y,
        video_width: video.width,
        video_height: video.height,
        overlay_scale_percent,
        area: ImageArea {
            x: 0,
            y: 0,
            cols,
            rows,
        },
    }
}

fn target_for_bounds(
    source_width: u32,
    source_height: u32,
    pixel_width: u32,
    pixel_height: u32,
) -> TargetFrame {
    let max_width = pixel_width.min(MAX_DECODE_WIDTH).min(source_width).max(1);
    let max_height = pixel_height
        .min(MAX_DECODE_HEIGHT)
        .min(source_height)
        .max(1);
    let capped = fit_pixels(source_width, source_height, max_width, max_height);

    TargetFrame {
        width: capped.width.max(1),
        height: capped.height.max(1),
    }
}

#[derive(Clone, Copy)]
struct PixelSize {
    width: u32,
    height: u32,
}

fn fit_pixels(source_width: u32, source_height: u32, max_width: u32, max_height: u32) -> PixelSize {
    let source_aspect = f64::from(source_width.max(1)) / f64::from(source_height.max(1));
    let max_aspect = f64::from(max_width.max(1)) / f64::from(max_height.max(1));

    let (width, height) = if max_aspect > source_aspect {
        (
            (f64::from(max_height) * source_aspect).round() as u32,
            max_height,
        )
    } else {
        (
            max_width,
            (f64::from(max_width) / source_aspect).round() as u32,
        )
    };

    PixelSize {
        width: width.max(1),
        height: height.max(1),
    }
}

fn cap_pixels(width: u32, height: u32, max_width: u32, max_height: u32) -> PixelSize {
    fit_pixels(
        width,
        height,
        width.min(max_width).max(1),
        height.min(max_height).max(1),
    )
}

fn overlay_scale_percent(
    pixel_width: u32,
    pixel_height: u32,
    canvas_width: u32,
    canvas_height: u32,
) -> u32 {
    let width_scale = f64::from(pixel_width.max(1)) / f64::from(canvas_width.max(1));
    let height_scale = f64::from(pixel_height.max(1)) / f64::from(canvas_height.max(1));
    let canvas_scale = width_scale.max(height_scale).max(1.0);
    let boost = ((canvas_scale - 1.0) * 40.0).round() as u32;

    NORMAL_OVERLAY_SCALE_PERCENT
        .saturating_add(boost)
        .clamp(NORMAL_OVERLAY_SCALE_PERCENT, MAX_OVERLAY_SCALE_PERCENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_caps_large_sources_at_1080p() {
        let target = target_for_bounds(3840, 2160, 3840, 2160);
        assert_eq!(target.width, 1920);
        assert_eq!(target.height, 1080);
    }

    #[test]
    fn target_does_not_upscale_small_sources() {
        let target = target_for_bounds(1280, 720, 3840, 2160);
        assert_eq!(target.width, 1280);
        assert_eq!(target.height, 720);
    }

    #[test]
    fn target_preserves_aspect_inside_1080p_cap() {
        let target = target_for_bounds(2560, 1080, 3840, 2160);
        assert_eq!(target.width, 1920);
        assert_eq!(target.height, 810);
    }

    #[test]
    fn canvas_uses_terminal_letterbox_space() {
        let canvas = canvas_for_terminal(1280, 536, 80, 24, 1920, 1080);
        assert_eq!(
            canvas,
            CanvasFrame {
                width: 1920,
                height: 1080,
                terminal_width: 1920,
                terminal_height: 1080,
                video_x: 0,
                video_y: 138,
                video_width: 1920,
                video_height: 804,
                overlay_scale_percent: 100,
                area: ImageArea {
                    x: 0,
                    y: 0,
                    cols: 80,
                    rows: 24,
                },
            }
        );
    }

    #[test]
    fn canvas_caps_high_density_terminals() {
        let canvas = canvas_for_terminal(1280, 536, 120, 40, 2880, 1800);
        assert_eq!(
            canvas,
            CanvasFrame {
                width: 1920,
                height: 1200,
                terminal_width: 2880,
                terminal_height: 1800,
                video_x: 0,
                video_y: 198,
                video_width: 1920,
                video_height: 804,
                overlay_scale_percent: 120,
                area: ImageArea {
                    x: 0,
                    y: 0,
                    cols: 120,
                    rows: 40,
                },
            }
        );
    }
}
