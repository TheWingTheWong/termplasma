//! Starfield: points in 3D flying toward the camera.
//!
//! Each star has a position in a unit cube in front of the viewer. Every
//! frame its depth `z` shrinks and it is projected with the perspective
//! divide `screen = center + xy / z * focal`. Stars that pass the near plane
//! or leave the screen respawn at the far plane. Brightness comes from depth,
//! and each star is drawn as a line from last frame's projected position to
//! this one, which turns fast close stars into streaks. The whole field rolls
//! slowly around the view axis and the speed swells and eases, the hyperspace
//! jump that opened half the intros of the early 90s.

use super::{Effect, Frame};
use crate::color::Rgb;
use crate::framebuffer::FrameBuffer;
use crate::lut::{fcos, fsin};
use crate::palette::Palette;
use crate::rng::Rng;

const NEAR: f32 = 0.03;
/// Longest streak, in pixels, so a star at the near plane cannot paint a
/// line across the whole screen.
const MAX_STREAK: f32 = 24.0;

#[derive(Clone, Copy)]
struct Star {
    x: f32,
    y: f32,
    z: f32,
    prev: Option<(f32, f32)>,
}

pub struct Starfield {
    stars: Vec<Star>,
    rng: Rng,
    size: (usize, usize),
}

impl Default for Starfield {
    fn default() -> Self {
        Starfield { stars: Vec::new(), rng: Rng::new(0x5_7A25), size: (0, 0) }
    }
}

impl Starfield {
    fn spawn(rng: &mut Rng, z: f32) -> Star {
        Star { x: rng.signed(), y: rng.signed(), z, prev: None }
    }

    fn rebuild(&mut self, w: usize, h: usize) {
        self.size = (w, h);
        let count = (w * h / 24).clamp(32, 2000);
        self.stars = (0..count)
            .map(|_| {
                let z = NEAR + self.rng.unit() * (1.0 - NEAR);
                Starfield::spawn(&mut self.rng, z)
            })
            .collect();
    }
}

impl Effect for Starfield {
    fn name(&self) -> &'static str {
        "starfield"
    }

    fn palette(&self) -> &'static str {
        "copper"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if (w, h) != self.size {
            self.rebuild(w, h);
        }
        fb.clear(palette.get(0));
        if w == 0 || h == 0 {
            return;
        }

        let t = frame.time;
        let speed = 0.5 + 0.45 * (1.0 + fsin(t * 0.45 - 1.2));
        let (sin_r, cos_r) = (fsin(t * 0.12), fcos(t * 0.12));
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let focal = w.max(h) as f32 * 0.5;

        for i in 0..self.stars.len() {
            let mut s = self.stars[i];
            s.z -= speed * frame.dt;
            if s.z <= NEAR {
                s = Starfield::spawn(&mut self.rng, 1.0);
            }
            let rx = s.x * cos_r - s.y * sin_r;
            let ry = s.x * sin_r + s.y * cos_r;
            let sx = cx + rx / s.z * focal;
            let sy = cy + ry / s.z * focal;
            if sx < -1.0 || sy < -1.0 || sx > w as f32 || sy > h as f32 {
                self.stars[i] = Starfield::spawn(&mut self.rng, 1.0);
                continue;
            }

            let depth = 1.0 - s.z;
            let bright = (depth * (2.0 - depth) * 255.0) as u8;
            let head = palette.get(bright.max(70));
            match s.prev {
                Some((px, py)) => {
                    line(fb, (px, py), (sx, sy), palette.get((bright as u16 * 2 / 3) as u8), head)
                }
                None => fb.set(sx as i32, sy as i32, head),
            }
            if s.z < 0.3 {
                // Close stars get a 2x2 head so they read as nearer.
                for (ox, oy) in [(1, 0), (0, 1), (1, 1)] {
                    fb.set(sx as i32 + ox, sy as i32 + oy, head);
                }
            }
            s.prev = Some((sx, sy));
            self.stars[i] = s;
        }
    }
}

/// Draws a streak from `a` (tail color) to `b` (head color), clamped to
/// [`MAX_STREAK`] pixels measured back from the head.
fn line(fb: &mut FrameBuffer, a: (f32, f32), b: (f32, f32), tail: Rgb, head: Rgb) {
    let (mut dx, mut dy) = (a.0 - b.0, a.1 - b.1);
    let len = dx.abs().max(dy.abs());
    if len > MAX_STREAK {
        dx *= MAX_STREAK / len;
        dy *= MAX_STREAK / len;
    }
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
    for i in 0..=steps {
        // i = 0 is the head, i = steps the far end of the tail.
        let f = i as f32 / steps as f32;
        let x = b.0 + dx * f;
        let y = b.1 + dy * f;
        let c = head.mix(tail, (f * 256.0) as u16);
        fb.set(x as i32, y as i32, c);
    }
}
