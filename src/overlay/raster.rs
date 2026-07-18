//! Low-level RGB shape rasterization and alpha blending.

#[derive(Clone, Copy)]
pub(super) struct RoundedRect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) radius: f64,
}

#[derive(Clone, Copy)]
pub(super) struct Circle {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) radius: f64,
}

#[derive(Clone, Copy)]
pub(super) struct Point {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Clone, Copy)]
pub(super) struct Triangle {
    pub(super) a: Point,
    pub(super) b: Point,
    pub(super) c: Point,
}

pub(super) fn fill_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    let min_x = rect.x.floor().max(0.0) as u32;
    let max_x = (rect.x + rect.width).ceil().min(f64::from(width)) as u32;
    let min_y = rect.y.floor().max(0.0) as u32;
    let max_y = (rect.y + rect.height).ceil().min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let coverage = rounded_rect_coverage(f64::from(x) + 0.5, f64::from(y) + 0.5, rect);
            if coverage > 0.0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(
                    frame,
                    offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
}

pub(super) fn stroke_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: RoundedRect,
    stroke: f64,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 || rect.width <= 0.0 || rect.height <= 0.0 || stroke <= 0.0 {
        return;
    }

    let stroke = stroke.min(rect.width / 2.0).min(rect.height / 2.0);
    let inner = RoundedRect {
        x: rect.x + stroke,
        y: rect.y + stroke,
        width: rect.width - stroke * 2.0,
        height: rect.height - stroke * 2.0,
        radius: (rect.radius - stroke / 2.0).max(0.0),
    };
    let min_x = rect.x.floor().max(0.0) as u32;
    let max_x = (rect.x + rect.width).ceil().min(f64::from(width)) as u32;
    let min_y = rect.y.floor().max(0.0) as u32;
    let max_y = (rect.y + rect.height).ceil().min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let x = f64::from(x) + 0.5;
            let y = f64::from(y) + 0.5;
            let coverage = (rounded_rect_coverage(x, y, rect) - rounded_rect_coverage(x, y, inner))
                .clamp(0.0, 1.0);
            if coverage > 0.0 {
                let offset = rgb_offset(width, x.floor() as u32, y.floor() as u32);
                blend_pixel(
                    frame,
                    offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
}

pub(super) fn fill_circle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    circle: Circle,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 || circle.radius <= 0.0 {
        return;
    }

    let min_x = (circle.x - circle.radius - 1.0).floor().max(0.0) as u32;
    let max_x = (circle.x + circle.radius + 1.0)
        .ceil()
        .min(f64::from(width)) as u32;
    let min_y = (circle.y - circle.radius - 1.0).floor().max(0.0) as u32;
    let max_y = (circle.y + circle.radius + 1.0)
        .ceil()
        .min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let dx = f64::from(x) + 0.5 - circle.x;
            let dy = f64::from(y) + 0.5 - circle.y;
            let coverage = (circle.radius - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(
                    frame,
                    offset,
                    color,
                    (coverage * f64::from(alpha)).round() as u8,
                );
            }
        }
    }
}

pub(super) fn fill_triangle(
    frame: &mut [u8],
    width: u32,
    height: u32,
    triangle: Triangle,
    color: [u8; 3],
    alpha: u8,
) {
    if width == 0 || height == 0 {
        return;
    }

    let min_x = triangle
        .a
        .x
        .min(triangle.b.x)
        .min(triangle.c.x)
        .floor()
        .max(0.0) as u32;
    let max_x = triangle
        .a
        .x
        .max(triangle.b.x)
        .max(triangle.c.x)
        .ceil()
        .min(f64::from(width)) as u32;
    let min_y = triangle
        .a
        .y
        .min(triangle.b.y)
        .min(triangle.c.y)
        .floor()
        .max(0.0) as u32;
    let max_y = triangle
        .a
        .y
        .max(triangle.b.y)
        .max(triangle.c.y)
        .ceil()
        .min(f64::from(height)) as u32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let mut covered = 0_u32;
            for sample_y in [0.25, 0.75] {
                for sample_x in [0.25, 0.75] {
                    if triangle_contains(
                        triangle,
                        Point {
                            x: f64::from(x) + sample_x,
                            y: f64::from(y) + sample_y,
                        },
                    ) {
                        covered += 1;
                    }
                }
            }
            if covered > 0 {
                let offset = rgb_offset(width, x, y);
                blend_pixel(frame, offset, color, (u32::from(alpha) * covered / 4) as u8);
            }
        }
    }
}

fn triangle_contains(triangle: Triangle, point: Point) -> bool {
    let d1 = edge_sign(point, triangle.a, triangle.b);
    let d2 = edge_sign(point, triangle.b, triangle.c);
    let d3 = edge_sign(point, triangle.c, triangle.a);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

fn edge_sign(point: Point, a: Point, b: Point) -> f64 {
    (point.x - b.x) * (a.y - b.y) - (a.x - b.x) * (point.y - b.y)
}

pub(super) fn rounded_rect_coverage(x: f64, y: f64, rect: RoundedRect) -> f64 {
    let half_width = rect.width / 2.0;
    let half_height = rect.height / 2.0;
    let radius = rect.radius.min(half_width).min(half_height);
    let center_x = rect.x + half_width;
    let center_y = rect.y + half_height;
    let qx = (x - center_x).abs() - (half_width - radius);
    let qy = (y - center_y).abs() - (half_height - radius);
    let outside_x = qx.max(0.0);
    let outside_y = qy.max(0.0);
    let distance =
        (outside_x * outside_x + outside_y * outside_y).sqrt() + qx.max(qy).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fill_solid_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    cols: u32,
    rows: u32,
    color: [u8; 3],
    alpha: u8,
) {
    for py in y..y.saturating_add(rows).min(height) {
        for px in x..x.saturating_add(cols).min(width) {
            let offset = rgb_offset(width, px, py);
            blend_pixel(frame, offset, color, alpha);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn blend_pixel(frame: &mut [u8], offset: usize, color: [u8; 3], alpha: u8) {
    let inverse = u16::from(255 - alpha);
    let alpha = u16::from(alpha);
    for channel in 0..3 {
        let source = u16::from(color[channel]) * alpha;
        let dest = u16::from(frame[offset + channel]) * inverse;
        frame[offset + channel] = ((source + dest + 127) / 255) as u8;
    }
}

pub(super) fn rgb_offset(width: u32, x: u32, y: u32) -> usize {
    ((y * width + x) * 3) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::style::TEXT_COLOR;

    #[test]
    fn rounded_rect_stroke_preserves_inner_pixels() {
        let width = 32;
        let height = 20;
        let mut frame = vec![20_u8; (width * height * 3) as usize];

        stroke_rounded_rect(
            &mut frame,
            width,
            height,
            RoundedRect {
                x: 4.0,
                y: 4.0,
                width: 24.0,
                height: 12.0,
                radius: 3.0,
            },
            2.0,
            TEXT_COLOR,
            255,
        );

        let border = rgb_offset(width, 4, 10);
        assert_eq!(&frame[border..border + 3], &TEXT_COLOR);

        let inner = rgb_offset(width, 16, 10);
        assert_eq!(&frame[inner..inner + 3], &[20, 20, 20]);
    }
}
