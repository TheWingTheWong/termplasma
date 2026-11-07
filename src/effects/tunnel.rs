//! Tunnel: a texture wrapped around the inside of a cylinder, using the
//! angle/distance table technique.
//!
//! For every pixel, two values are computed once per resize: the angle
//! around the tunnel axis (`atan2`, mapped to texture u) and the depth
//! (`k / radius`, mapped to texture v). Rendering a frame is then two table
//! reads, two adds for motion (scrolling depth moves you forward, scrolling
//! angle rotates the tunnel) and one texture fetch per pixel. A third table
//! holds a depth fog so the far end fades into the palette's darkest color.
//!
//! The tables are twice the screen size and sampled through a moving window,
//! which shifts the vanishing point around: the classic "looking around the
//! bend" trick that needs no extra math. The texture is the XOR pattern,
//! `u ^ v`, the most-used procedural texture in demo history.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::fsin;
use crate::palette::Palette;

const TEX: usize = 256;

pub struct Tunnel {
    w: usize,
    h: usize,
    depth: Vec<u8>,
    angle: Vec<u8>,
    fog: Vec<u8>,
    texture: Vec<u8>,
}

impl Default for Tunnel {
    fn default() -> Self {
        let mut texture = vec![0u8; TEX * TEX];
        for v in 0..TEX {
            for u in 0..TEX {
                // XOR pattern without its three lowest bits (they only add
                // texel-sized grain that aliases at terminal resolution),
                // folded so it has bright seams rather than a hard
                // 255 -> 0 drop every 256 texels.
                let x = ((u ^ v) & 0xf8) as u8;
                texture[v * TEX + u] = if x < 128 { x << 1 } else { (255 - x) << 1 };
            }
        }
        Tunnel { w: 0, h: 0, depth: Vec::new(), angle: Vec::new(), fog: Vec::new(), texture }
    }
}

impl Tunnel {
    fn rebuild(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        let (tw, th) = (w * 2, h * 2);
        let m = w.min(h).max(1) as f32;
        let n = tw * th;
        self.depth = Vec::with_capacity(n);
        self.angle = Vec::with_capacity(n);
        self.fog = Vec::with_capacity(n);
        for y in 0..th {
            let dy = y as f32 - h as f32 + 0.5;
            for x in 0..tw {
                let dx = x as f32 - w as f32 + 0.5;
                let r = (dx * dx + dy * dy).sqrt().max(0.5);
                self.depth.push((32.0 * m / r) as i32 as u8);
                let a = dy.atan2(dx) / std::f32::consts::TAU; // -0.5..0.5
                self.angle.push((a * 2.0 * TEX as f32) as i32 as u8);
                let f = (r / (0.55 * m)).min(1.0);
                self.fog.push((f.sqrt() * 256.0).min(255.0) as u8);
            }
        }
    }
}

impl Effect for Tunnel {
    fn name(&self) -> &'static str {
        "tunnel"
    }

    fn palette(&self) -> &'static str {
        "amber"
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
        let forward = (t * 90.0) as i32 as u8;
        let spin = (t * 22.0 + 40.0 * fsin(t * 0.3)) as i32 as u8;
        let ox = ((w - 1) as f32 * (0.5 + 0.3 * fsin(t * 0.47))) as usize;
        let oy = ((h - 1) as f32 * (0.5 + 0.3 * fsin(t * 0.61 + 0.8))) as usize;
        let tw = w * 2;

        for y in 0..h {
            let start = (y + oy) * tw + ox;
            let depth = &self.depth[start..start + w];
            let angle = &self.angle[start..start + w];
            let fog = &self.fog[start..start + w];
            let row = fb.row_mut(y);
            for (i, px) in row.iter_mut().enumerate() {
                let v = depth[i].wrapping_add(forward) as usize;
                let u = angle[i].wrapping_add(spin) as usize;
                let texel = self.texture[v * TEX + u] as u32;
                *px = palette.get(((texel * fog[i] as u32) >> 8) as u8);
            }
        }
    }
}
