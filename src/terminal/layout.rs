#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ImageArea {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

pub(crate) fn image_area_for_terminal(
    source_width: u32,
    source_height: u32,
    cols: u16,
    rows: u16,
    pixel_width: u32,
    pixel_height: u32,
) -> ImageArea {
    let cell_width = f64::from(pixel_width) / f64::from(cols.max(1));
    let cell_height = f64::from(pixel_height) / f64::from(rows.max(1));
    fit_image_area(
        source_width,
        source_height,
        cols,
        rows,
        cell_width,
        cell_height,
    )
}

fn fit_image_area(
    source_width: u32,
    source_height: u32,
    cols: u16,
    rows: u16,
    cell_width: f64,
    cell_height: f64,
) -> ImageArea {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let max_width_px = f64::from(cols) * cell_width;
    let max_height_px = f64::from(rows) * cell_height;
    let source_aspect = f64::from(source_width.max(1)) / f64::from(source_height.max(1));

    let (display_width_px, display_height_px) = if max_width_px / max_height_px > source_aspect {
        (max_height_px * source_aspect, max_height_px)
    } else {
        (max_width_px, max_width_px / source_aspect)
    };

    let display_cols = ((display_width_px / cell_width).floor() as u16).clamp(1, cols);
    let display_rows = ((display_height_px / cell_height).floor() as u16).clamp(1, rows);

    ImageArea {
        x: cols.saturating_sub(display_cols) / 2,
        y: rows.saturating_sub(display_rows) / 2,
        cols: display_cols,
        rows: display_rows,
    }
}

pub(crate) fn terminal_pixel_size(cols: u16, rows: u16) -> (u32, u32) {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } == 0;
    if ok {
        let size = unsafe { size.assume_init() };
        if size.ws_xpixel > 0 && size.ws_ypixel > 0 {
            return (u32::from(size.ws_xpixel), u32::from(size.ws_ypixel));
        }
    }

    (u32::from(cols.max(1)) * 8, u32::from(rows.max(1)) * 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_area_preserves_source_aspect() {
        let area = fit_image_area(1920, 1080, 80, 24, 10.0, 20.0);

        assert_eq!(area.cols, 80);
        assert_eq!(area.rows, 22);
        assert_eq!(area.x, 0);
        assert_eq!(area.y, 1);
    }
}
