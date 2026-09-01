use crate::overlay::{HitboxRect, OverlayHitPoint};

use super::layout::CanvasFrame;

pub(super) fn canvas_position(
    column: u16,
    row: u16,
    canvas: CanvasFrame,
) -> Option<OverlayHitPoint> {
    if coordinates_are_pixels(column, row, canvas) {
        let x = pixel_to_canvas(u32::from(column), canvas.terminal_width, canvas.width);
        let y = pixel_to_canvas(u32::from(row), canvas.terminal_height, canvas.height);
        return Some(OverlayHitPoint {
            x,
            y,
            cell: HitboxRect {
                left: x,
                top: y,
                right: x,
                bottom: y,
            },
        });
    }

    let end_col = canvas.area.x.saturating_add(canvas.area.cols);
    let end_row = canvas.area.y.saturating_add(canvas.area.rows);
    if column < canvas.area.x || column >= end_col || row < canvas.area.y || row >= end_row {
        return None;
    }

    let rel_col = column - canvas.area.x;
    let rel_row = row - canvas.area.y;
    let x = cell_to_pixel(rel_col, canvas.area.cols, canvas.width);
    let y = cell_to_pixel(rel_row, canvas.area.rows, canvas.height);
    let (left, right) = cell_pixel_bounds(rel_col, canvas.area.cols, canvas.width);
    let (top, bottom) = cell_pixel_bounds(rel_row, canvas.area.rows, canvas.height);

    Some(OverlayHitPoint {
        x,
        y,
        cell: HitboxRect {
            left,
            top,
            right,
            bottom,
        },
    })
}

pub(super) fn canvas_x(column: u16, row: u16, canvas: CanvasFrame) -> u32 {
    if coordinates_are_pixels(column, row, canvas) {
        return pixel_to_canvas(u32::from(column), canvas.terminal_width, canvas.width);
    }

    let rel = if column <= canvas.area.x {
        0
    } else {
        column
            .saturating_sub(canvas.area.x)
            .min(canvas.area.cols.saturating_sub(1))
    };
    cell_to_pixel(rel, canvas.area.cols, canvas.width)
}

fn coordinates_are_pixels(column: u16, row: u16, canvas: CanvasFrame) -> bool {
    column >= canvas.area.cols || row >= canvas.area.rows
}

fn cell_to_pixel(cell: u16, cells: u16, pixels: u32) -> u32 {
    let cells = f64::from(cells.max(1));
    let pixels = pixels.max(1);
    (((f64::from(cell) + 0.5) * f64::from(pixels)) / cells)
        .floor()
        .min(f64::from(pixels - 1)) as u32
}

fn cell_pixel_bounds(cell: u16, cells: u16, pixels: u32) -> (u32, u32) {
    let cells = u64::from(cells.max(1));
    let pixels = u64::from(pixels.max(1));
    let cell = u64::from(cell).min(cells.saturating_sub(1));
    let left = cell.saturating_mul(pixels) / cells;
    let right = (cell.saturating_add(1))
        .saturating_mul(pixels)
        .div_ceil(cells)
        .saturating_sub(1)
        .min(pixels.saturating_sub(1));
    (left as u32, right as u32)
}

fn pixel_to_canvas(pixel: u32, terminal_pixels: u32, canvas_pixels: u32) -> u32 {
    let terminal_pixels = terminal_pixels.max(1);
    let canvas_pixels = canvas_pixels.max(1);
    (u64::from(pixel.min(terminal_pixels.saturating_sub(1))) * u64::from(canvas_pixels)
        / u64::from(terminal_pixels))
    .min(u64::from(canvas_pixels - 1)) as u32
}
