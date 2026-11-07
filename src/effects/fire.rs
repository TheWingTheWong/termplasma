//! Fire: the cellular-automaton flame from the PlayStation/N64 ports of Doom.
//!
//! The bottom row is held at full heat. Every step, each pixel copies the
//! heat of the pixel directly below it, shifted one column left or right at
//! random and minus a small random decay. Heat that rises far enough from the
//! source fades to zero, and the random sideways copy makes the tongues
//! flicker and lean. The heat value is used directly as a palette index,
//! which is why the PSX fire ramp is the natural palette for it.
//!
//! Like Doom, heat has only 37 levels (0..=36), and that coarseness is what
//! produces coherent tongues instead of noise. Doom's decay was a coin flip
//! of 0 or 1 per row on a fixed 200-line screen; here the mean decay is
//! scaled to the frame height so the flames climb the same proportion of the
//! screen at any terminal size. The simulation runs on a fixed 60 Hz clock,
//! independent of the render frame rate.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::fsin;
use crate::palette::Palette;
use crate::rng::Rng;

const STEP_HZ: f32 = 60.0;
/// Hottest heat level, as in Doom.
const MAX_HEAT: u8 = 36;
/// Fraction of the screen height a flame climbs on average.
const REACH: f32 = 0.62;

pub struct Fire {
    w: usize,
    h: usize,
    heat: Vec<u8>,
    rng: Rng,
    /// Mean decay per row in 8.8 fixed point; random decay is drawn from
    /// `0..=2*mean` so the average matches.
    decay_q8: u32,
    accum: f32,
    steps: u64,
}

impl Default for Fire {
    fn default() -> Self {
        Fire { w: 0, h: 0, heat: Vec::new(), rng: Rng::new(0xF1AE), decay_q8: 0, accum: 0.0, steps: 0 }
    }
}

impl Fire {
    fn rebuild(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        self.heat = vec![0; w * h];
        self.decay_q8 = ((MAX_HEAT as f32 * 256.0) / (REACH * h as f32)).max(1.0) as u32;
        // Pre-warm so the fire is already burning when it first appears.
        for _ in 0..h + 8 {
            self.step();
        }
    }

    fn step(&mut self) {
        let (w, h) = (self.w, self.h);
        // Doom held the source row at a constant maximum. Modulating it with
        // two slow travelling sine waves (plus rare cold sparks) gives the
        // flame body large, drifting tongues instead of an even curtain.
        let base = (h - 1) * w;
        let t = self.steps as f32 / STEP_HZ;
        let k = 64.0 / w.max(1) as f32;
        for x in 0..w {
            let xf = x as f32 * k;
            let wave = fsin(xf * 0.9 + t * 1.7) + fsin(xf * 0.37 - t * 1.1);
            let cool = ((1.0 - wave) * 0.25 * 11.0) as u8;
            let spark = if self.rng.below(24) == 0 { 6 } else { 0 };
            self.heat[base + x] = MAX_HEAT - cool.min(14) - spark;
        }
        let span = 2 * self.decay_q8 + 1;
        for y in 1..h {
            for x in 0..w {
                let src = self.heat[y * w + x];
                let r = self.rng.next_u32();
                // Doom's drift: -2..=+1 columns, biased so the flames lean
                // into a gentle wind. Columns wrap like Doom's linear buffer.
                let dx = 1 - (r & 3) as isize;
                let dst_x = (x as isize + dx).rem_euclid(w as isize) as usize;
                let decay = ((((r >> 8) % span) + 128) >> 8) as u8;
                self.heat[(y - 1) * w + dst_x] = src.saturating_sub(decay);
            }
        }
        self.steps += 1;
    }
}

impl Effect for Fire {
    fn name(&self) -> &'static str {
        "fire"
    }

    fn palette(&self) -> &'static str {
        "doom"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if w == 0 || h == 0 {
            return;
        }
        if (w, h) != (self.w, self.h) {
            self.rebuild(w, h);
        }
        // Fixed-step simulation; cap catch-up so a stall cannot spiral.
        self.accum += frame.dt * STEP_HZ;
        let mut n = 0;
        while self.accum >= 1.0 && n < 4 {
            self.step();
            self.accum -= 1.0;
            n += 1;
        }
        if n == 4 {
            self.accum = self.accum.min(1.0);
        }
        // Spread the 37 levels over the full palette.
        let mut lut = [0u8; MAX_HEAT as usize + 1];
        for (level, slot) in lut.iter_mut().enumerate() {
            *slot = (level * 255 / MAX_HEAT as usize) as u8;
        }
        for (px, &heat) in fb.pixels_mut().iter_mut().zip(&self.heat) {
            *px = palette.get(lut[heat as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_is_hot_and_top_is_cold() {
        let mut f = Fire::default();
        f.rebuild(64, 80);
        let bottom: u32 = f.heat[79 * 64..].iter().map(|&v| v as u32).sum();
        let top: u32 = f.heat[..64].iter().map(|&v| v as u32).sum();
        assert!(bottom > 64 * 30, "source row stays hot");
        assert!(top < 64 * 3, "flames die out before the top: {top}");
    }

    #[test]
    fn fixed_timestep_is_frame_rate_independent() {
        let pal = crate::palette::builtin().remove(1);
        let mut a = Fire::default();
        let mut b = Fire::default();
        let mut fb = FrameBuffer::new(16, 16);
        // Power-of-two frame times keep the float accumulator exact.
        for _ in 0..64 {
            a.render(&mut fb, Frame { time: 0.0, dt: 1.0 / 64.0 }, &pal);
        }
        for _ in 0..32 {
            b.render(&mut fb, Frame { time: 0.0, dt: 1.0 / 32.0 }, &pal);
        }
        assert_eq!(a.steps, b.steps);
    }

    #[test]
    fn paused_fire_does_not_advance() {
        let pal = crate::palette::builtin().remove(1);
        let mut f = Fire::default();
        let mut fb = FrameBuffer::new(8, 8);
        f.render(&mut fb, Frame::default(), &pal);
        let before = f.steps;
        f.render(&mut fb, Frame::default(), &pal);
        assert_eq!(f.steps, before);
    }
}
