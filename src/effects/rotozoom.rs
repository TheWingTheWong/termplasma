//! Rotozoomer: a tiled texture rotated and scaled in real time, using only
//! integer adds in the inner loop.
//!
//! Rotation plus zoom is an affine map, so moving one pixel right on screen
//! always moves the texture coordinate by the same vector `(du, dv)`, and
//! moving one row down moves it by the perpendicular vector `(-dv, du)`.
//! Both are computed once per frame from the fixed-point sine table in
//! 16.16 format. Each scanline starts from the previous one plus the row
//! step, and each pixel is the previous plus the column step; the texel is
//! the integer part masked to the 256x256 texture. Wrapping arithmetic is
//! intentional: only bits 16..24 are ever used, so overflow is harmless.
//!
//! The texture is procedural: a checkerboard of bevelled tiles with bright
//! grout lines and a faint diagonal sheen, built once at startup.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::{fsin, icos, isin, to_angle};
use crate::palette::Palette;

const TEX: usize = 256;
const TILE: usize = 32;

pub struct Rotozoom {
    texture: Vec<u8>,
}

impl Default for Rotozoom {
    fn default() -> Self {
        let mut texture = vec![0u8; TEX * TEX];
        for v in 0..TEX {
            for u in 0..TEX {
                let (lu, lv) = (u % TILE, v % TILE);
                // Distance to the nearest tile edge, 0..=15.
                let edge = lu.min(TILE - 1 - lu).min(lv).min(TILE - 1 - lv);
                let dark = ((u / TILE) ^ (v / TILE)) & 1 == 1;
                let bevel = (edge.min(6) * 9) as u8;
                let sheen = (((u + v) % 64) / 8) as u8;
                texture[v * TEX + u] = if edge == 0 {
                    255
                } else if dark {
                    40 + bevel / 2 + sheen
                } else {
                    150 + bevel + sheen
                };
            }
        }
        Rotozoom { texture }
    }
}

impl Effect for Rotozoom {
    fn name(&self) -> &'static str {
        "rotozoom"
    }

    fn palette(&self) -> &'static str {
        "c64"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if w == 0 || h == 0 {
            return;
        }
        let t = frame.time;
        let angle = to_angle(t * 0.45 + 0.6 * fsin(t * 0.21));
        // Texels per screen pixel: about four tiles across the short side at
        // zoom 1, breathing between close-up and far away.
        let zoom = 1.0 + 0.6 * fsin(t * 0.37);
        let scale = (zoom * (4.0 * TILE as f32) / w.min(h) as f32 * 65536.0) as i64;
        let du = ((icos(angle) as i64 * scale) >> 16) as i32;
        let dv = ((isin(angle) as i64 * scale) >> 16) as i32;

        // Texture point under the screen center drifts so the pattern pans.
        let cu = (t * 40.0 * 65536.0) as i64 as i32;
        let cv = (t * 23.0 * 65536.0) as i64 as i32;
        let (hx, hy) = ((w / 2) as i32, (h / 2) as i32);
        // Top-left corner: center minus half a screen along both axes.
        let mut row_u = cu.wrapping_sub(du.wrapping_mul(hx)).wrapping_add(dv.wrapping_mul(hy));
        let mut row_v = cv.wrapping_sub(dv.wrapping_mul(hx)).wrapping_sub(du.wrapping_mul(hy));

        for y in 0..h {
            let (mut u, mut v) = (row_u, row_v);
            for px in fb.row_mut(y) {
                let ti = ((v >> 16) as usize & (TEX - 1)) * TEX + ((u >> 16) as usize & (TEX - 1));
                *px = palette.get(self.texture[ti]);
                u = u.wrapping_add(du);
                v = v.wrapping_add(dv);
            }
            row_u = row_u.wrapping_sub(dv);
            row_v = row_v.wrapping_add(du);
        }
    }
}
