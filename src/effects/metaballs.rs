//! Metaballs: an implicit surface from summed inverse-square fields.
//!
//! Each ball contributes `r^2 / d^2` at every pixel; where the sum crosses
//! 1.0 is the surface, and because the fields add, nearby balls bulge toward
//! each other and merge like mercury. The squared distance splits into
//! `dx^2 + dy^2`, so per frame each ball gets one table of `dx^2` per column
//! and one of `dy^2` per row; the per-pixel cost is then an add and a
//! division per ball.
//!
//! The field is shaded in three bands: a soft glow outside the surface, a
//! thin bright rim right at the threshold, and a body that brightens toward
//! each ball's core.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::{fcos, fsin};
use crate::palette::Palette;

/// (x frequency, y frequency, phase, radius scale) for each ball's
/// Lissajous orbit.
const BALLS: [(f32, f32, f32, f32); 6] = [
    (0.53, 0.71, 0.0, 1.00),
    (0.37, 0.43, 1.7, 0.85),
    (0.61, 0.29, 3.1, 0.70),
    (0.23, 0.57, 4.4, 1.10),
    (0.47, 0.33, 5.2, 0.60),
    (0.31, 0.67, 2.4, 0.75),
];

#[derive(Default)]
pub struct Metaballs {
    dx2: Vec<f32>,
    dy2: Vec<f32>,
    r2: [f32; BALLS.len()],
}

impl Effect for Metaballs {
    fn name(&self) -> &'static str {
        "metaballs"
    }

    fn palette(&self) -> &'static str {
        "mercury"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if w == 0 || h == 0 {
            return;
        }
        let n = BALLS.len();
        self.dx2.resize(n * w, 0.0);
        self.dy2.resize(n * h, 0.0);
        let t = frame.time;
        let m = w.min(h) as f32;

        for (b, &(fx, fy, phase, scale)) in BALLS.iter().enumerate() {
            let cx = w as f32 * (0.5 + 0.36 * fsin(t * fx + phase));
            let cy = h as f32 * (0.5 + 0.34 * fcos(t * fy + phase * 1.3));
            let r = m * 0.13 * scale * (1.0 + 0.15 * fsin(t * 1.3 + phase));
            self.r2[b] = r * r;
            for x in 0..w {
                let d = x as f32 + 0.5 - cx;
                self.dx2[b * w + x] = d * d;
            }
            for y in 0..h {
                let d = y as f32 + 0.5 - cy;
                self.dy2[b * h + y] = d * d;
            }
        }

        for y in 0..h {
            let row = fb.row_mut(y);
            for (x, px) in row.iter_mut().enumerate() {
                let mut field = 0.0f32;
                for b in 0..n {
                    field += self.r2[b] / (self.dx2[b * w + x] + self.dy2[b * h + y] + 1.0);
                }
                let index = if field < 1.0 {
                    // Glow: quadratic falloff keeps the background dark.
                    (field * field * 150.0) as u8
                } else if field < 1.08 {
                    // Rim.
                    250
                } else {
                    (170.0 + (field - 1.0) * 28.0).min(240.0) as u8
                };
                *px = palette.get(index);
            }
        }
    }
}
