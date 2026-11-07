//! Fire: the cellular-automaton flame from the PlayStation/N64 ports of Doom.
//!
//! The bottom row is held at full heat. Every step, each pixel copies the
//! heat of the pixel directly below it, shifted one column left or right at
//! random and minus a small random decay. Heat that rises far enough from the
//! source fades to zero, and the random sideways copy makes the tongues
//! flicker and lean. The heat value is used directly as a palette index,
//! which is why the PSX fire ramp is the natural palette for it.
//!
//! Like Doom, heat has only 37 levels (0..=36) and the decay is Doom's coin
//! flip of 0 or 1 per row, drawn from the same random number as the drift.
//! That coarseness is what produces coherent tongues instead of noise, but it
//! also fixes how far a flame climbs: about 72 rows. So the automaton always
//! runs on a grid [`SIM_H`] rows tall (with the width chosen to keep the
//! screen's aspect ratio) and is resampled to the frame. The flames look the
//! same in a 20-row terminal and a 120-row one. The simulation steps on a
//! fixed 60 Hz clock, independent of the render frame rate.

use super::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::lut::fsin;
use crate::palette::Palette;
use crate::rng::Rng;

const STEP_HZ: f32 = 60.0;
/// Hottest heat level, as in Doom.
const MAX_HEAT: u8 = 36;
/// Simulation height: flames reach ~72 rows, so they fill about 60% of it.
const SIM_H: usize = 116;
/// Upper bound on the simulation width for very wide, short frames.
const MAX_SIM_W: usize = 1024;

pub struct Fire {
    size: (usize, usize),
    sim_w: usize,
    heat: Vec<u8>,
    /// Simulation column for each screen column.
    x_map: Vec<usize>,
    rng: Rng,
    accum: f32,
    steps: u64,
}

impl Default for Fire {
    fn default() -> Self {
        Fire {
            size: (0, 0),
            sim_w: 0,
            heat: Vec::new(),
            x_map: Vec::new(),
            rng: Rng::new(0xF1AE),
            accum: 0.0,
            steps: 0,
        }
    }
}

impl Fire {
    fn rebuild(&mut self, w: usize, h: usize) {
        self.size = (w, h);
        let sim_w = ((w * SIM_H + h / 2) / h).clamp(1, MAX_SIM_W);
        self.x_map = (0..w).map(|x| x * sim_w / w).collect();
        // Keep the old flames when only the height changed.
        if sim_w != self.sim_w {
            self.sim_w = sim_w;
            self.heat = vec![0; sim_w * SIM_H];
            // Pre-warm so the fire is already burning when it first appears.
            for _ in 0..SIM_H + 16 {
                self.step();
            }
        }
    }

    fn step(&mut self) {
        let w = self.sim_w;
        // Doom held the source row at a constant maximum. Modulating it with
        // two slow travelling sine waves (plus rare cold sparks) shapes the
        // body into large, drifting tongues instead of an even curtain.
        let base = (SIM_H - 1) * w;
        let t = self.steps as f32 / STEP_HZ;
        let k = 1.0 / SIM_H as f32;
        for x in 0..w {
            let xf = x as f32 * k;
            let wave = fsin(xf * 9.0 + t * 1.3) + fsin(xf * 3.7 - t * 0.9);
            let cool = ((1.2 - wave) * 7.0).clamp(0.0, 22.0) as u8;
            let spark = if self.rng.below(20) == 0 { 8 } else { 0 };
            self.heat[base + x] = MAX_HEAT - cool - spark;
        }
        for y in 1..SIM_H {
            for x in 0..w {
                let src = self.heat[y * w + x];
                let r = self.rng.next_u32();
                // Doom's spreadFire: drift of -2..=+1 columns (so the flames
                // lean into a gentle wind) and a decay of r & 1. Columns
                // wrap, as they did in Doom's linear buffer.
                let dst_x = (x + 2 * w + 1 - (r & 3) as usize) % w;
                self.heat[(y - 1) * w + dst_x] = src.saturating_sub((r & 1) as u8);
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
        if (w, h) != self.size {
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
        for y in 0..h {
            let sim_row = &self.heat[(y * SIM_H / h) * self.sim_w..][..self.sim_w];
            for (px, &sx) in fb.row_mut(y).iter_mut().zip(&self.x_map) {
                *px = palette.get(lut[sim_row[sx] as usize]);
            }
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
        let w = f.sim_w;
        assert_eq!(w, 93, "simulation keeps the frame's aspect ratio");
        let bottom: u32 = f.heat[(SIM_H - 1) * w..].iter().map(|&v| v as u32).sum();
        let top: u32 = f.heat[..w * 8].iter().map(|&v| v as u32).sum();
        assert!(bottom > w as u32 * 20, "source row stays hot");
        assert!(top < w as u32, "flames die out before the top: {top}");
    }

    #[test]
    fn fixed_timestep_is_frame_rate_independent() {
        let pal = crate::palette::builtin().remove(1);
        let mut a = Fire::default();
        let mut b = Fire::default();
        let mut fb = FrameBuffer::new(16, 16);
        a.render(&mut fb, Frame::default(), &pal);
        b.render(&mut fb, Frame::default(), &pal);
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
