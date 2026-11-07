//! Plasma: a sum of sine fields mapped through a cycling palette.
//!
//! The expensive terms (a radial wave and a warped interference wave) are
//! precomputed once per resize into tables twice the screen size. Each frame
//! samples a screen-sized window out of each table at an offset that drifts
//! on a Lissajous path, so the two layers slide over each other and the
//! pattern morphs without a single `sin` per pixel. Two cheap 1D terms (one
//! per column, one per row, also table lookups) add travelling bands.
//!
//! The four 8-bit terms are summed, folded into an index and offset by a
//! time-driven amount: that last add is palette cycling, the trick that made
//! VGA plasmas shimmer for free.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::{SIN_U8, fsin};
use crate::palette::Palette;

#[derive(Default)]
pub struct Plasma {
    w: usize,
    h: usize,
    radial: Vec<u8>,
    ripple: Vec<u8>,
    col_term: Vec<u8>,
    row_term: Vec<u8>,
}

impl Plasma {
    fn rebuild(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        let (tw, th) = (w * 2, h * 2);
        let m = w.min(h).max(1) as f32;
        let tau = std::f32::consts::TAU;
        self.radial = Vec::with_capacity(tw * th);
        self.ripple = Vec::with_capacity(tw * th);
        for y in 0..th {
            let dy = y as f32 - h as f32;
            for x in 0..tw {
                let dx = x as f32 - w as f32;
                let d = (dx * dx + dy * dy).sqrt();
                self.radial.push((127.5 + 127.5 * (d * tau / (m * 0.42)).sin()) as u8);
                let warp = (dx * tau / (m * 1.7)).sin() * 1.6;
                let v = (dx * tau / (m * 0.95)).sin() + (dy * tau / (m * 0.6) + warp).sin();
                self.ripple.push((127.5 + 63.75 * v) as u8);
            }
        }
        self.col_term = vec![0; w];
        self.row_term = vec![0; h];
    }
}

impl Effect for Plasma {
    fn name(&self) -> &'static str {
        "plasma"
    }

    fn palette(&self) -> &'static str {
        "velvet"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if w == 0 || h == 0 {
            return;
        }
        if (w, h) != (self.w, self.h) {
            self.rebuild(w, h);
        }
        let t = frame.time;
        let tw = w * 2;

        // Window offsets into the 2x tables; each stays within 0..w / 0..h.
        let path = |a: f32, b: f32, n: usize| ((n - 1) as f32 * (0.5 + 0.5 * fsin(t * a + b))) as usize;
        let (ax, ay) = (path(0.31, 0.0, w), path(0.23, 1.3, h));
        let (bx, by) = (path(0.17, 2.1, w), path(0.29, 0.4, h));

        let fx = 256.0 * 1.3 / w as f32;
        for (x, c) in self.col_term.iter_mut().enumerate() {
            *c = SIN_U8[((x as f32 * fx + t * 37.0) as i32 & 255) as usize];
        }
        let fy = 256.0 * 0.9 / h as f32;
        for (y, r) in self.row_term.iter_mut().enumerate() {
            *r = SIN_U8[((y as f32 * fy - t * 29.0) as i32 & 255) as usize];
        }

        let cycle = (t * 48.0) as i32 as u8;
        for y in 0..h {
            let ra = &self.radial[(y + ay) * tw + ax..][..w];
            let rb = &self.ripple[(y + by) * tw + bx..][..w];
            let rt = self.row_term[y] as u16;
            let out = fb.row_mut(y);
            for (((px, &a), &b), &c) in out.iter_mut().zip(ra).zip(rb).zip(&self.col_term) {
                let sum = a as u16 + b as u16 + c as u16 + rt;
                let index = ((sum >> 1) as u8).wrapping_add(cycle);
                *px = palette.cyclic(index);
            }
        }
    }
}
