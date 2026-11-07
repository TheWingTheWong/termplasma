//! Sine scroller: bitmap text crawling right to left on a sine wave, over
//! copper bars, with a rippling reflection below the horizon.
//!
//! Every screen column maps to one column of one glyph of the message; that
//! column is displaced vertically by a sine of its screen x plus time, so the
//! text rides a wave that holds still while the letters travel through it.
//! Glyphs come from the 8x8 font in [`crate::font`], scaled up by an integer
//! factor to suit the terminal height, and get a hard drop shadow.
//!
//! The copper bars imitate the Amiga copper coprocessor, which changed the
//! background color on chosen scanlines: each bar is a vertical intensity
//! profile looked up in the palette. Below the horizon every row copies a
//! mirrored row from above with a small sinusoidal sideways offset and a
//! darkening, the water reflection from countless cracktros.

use super::{Effect, Frame};
use crate::color::Rgb;
use crate::font::{GLYPH_SIZE, glyph};
use crate::framebuffer::FrameBuffer;
use crate::lut::fsin;
use crate::palette::Palette;

const MESSAGE: &str = "TERMPLASMA ... SEVEN CLASSIC EFFECTS IN A TERMINAL: TWO PIXELS PER \
    CHARACTER CELL, 24-BIT COLOR, AND ONLY THE CELLS THAT CHANGED ARE SENT ... FIRE AFTER \
    DOOM ON THE PLAYSTATION, PLASMA FROM SINE TABLES, A ROTOZOOMER IN 16.16 FIXED POINT, \
    A TUNNEL FROM ANGLE AND DEPTH TABLES ... KEYS: N NEXT, B BACK, P PALETTE, SPACE PAUSE, \
    H HELP, Q QUIT ... GREETINGS TO EVERYONE WHO EVER RACED THE BEAM ...      ";

/// Characters scrolled per second.
const CHARS_PER_SEC: f32 = 4.5;
/// Fraction of the height above the reflection.
const HORIZON: f32 = 0.74;

#[derive(Default)]
pub struct Scroller {
    /// Scratch row used while building the reflection.
    row: Vec<Rgb>,
}

impl Scroller {
    fn copper(fb: &mut FrameBuffer, horizon: usize, t: f32, palette: &Palette) {
        let w = fb.width();
        // Sky: a dark vertical gradient from the bottom of the palette.
        for y in 0..horizon {
            let c = palette.get((y * 36 / horizon.max(1)) as u8);
            fb.row_mut(y).fill(c);
        }
        let thick = (horizon as f32 / 14.0).max(1.5);
        for k in 0..4 {
            let center = horizon as f32 * (0.5 + 0.4 * fsin(t * 1.15 + k as f32 * 0.85));
            let y0 = (center - thick).floor().max(0.0) as usize;
            let y1 = ((center + thick).ceil() as usize).min(horizon);
            for y in y0..y1 {
                let d = (y as f32 + 0.5 - center) / thick;
                if d.abs() >= 1.0 {
                    continue;
                }
                let c = palette.get(((1.0 - d * d) * 235.0) as u8);
                let row = fb.row_mut(y);
                // Later bars pass in front only where they are brighter,
                // like overlapping copper lists.
                if row.first().is_some_and(|p| p.luma() < c.luma()) {
                    row[..w].fill(c);
                }
            }
        }
    }

    fn text(fb: &mut FrameBuffer, horizon: usize, t: f32, palette: &Palette) {
        let w = fb.width();
        let scale = (horizon / 22).clamp(1, 6);
        let cell = GLYPH_SIZE * scale;
        let text_h = cell as f32;
        let amp = ((horizon as f32 - text_h) * 0.5 - scale as f32 - 1.0).max(0.0) * 0.6;
        let base = (horizon as f32 - text_h) * 0.5;
        let chars: Vec<char> = MESSAGE.chars().collect();
        let loop_px = chars.len() * cell;
        let scroll = (t * CHARS_PER_SEC * cell as f32) as usize;
        let freq = std::f32::consts::TAU / (w as f32 * 0.7).max(8.0);

        // Shadow pass, then the letters, so shadows never cover text.
        for pass in 0..2 {
            let shadow = pass == 0;
            let off = if shadow { scale as i32 } else { 0 };
            for x in 0..w {
                let tx = (x + scroll) % loop_px;
                let bits = glyph(chars[tx / cell]);
                let col = (tx % cell) / scale;
                let y_top = base + amp * fsin(x as f32 * freq - t * 2.4);
                for (gy, &row_bits) in bits.iter().enumerate() {
                    if row_bits & (0x80 >> col) == 0 {
                        continue;
                    }
                    let color = if shadow {
                        Rgb::BLACK
                    } else {
                        // Bright crown fading down each glyph.
                        palette.get(255 - (gy * 13) as u8)
                    };
                    for sy in 0..scale {
                        let y = y_top as i32 + (gy * scale + sy) as i32 + off;
                        if (y as usize) < horizon {
                            fb.set(x as i32 + off, y, color);
                        }
                    }
                }
            }
        }
    }

    fn reflect(&mut self, fb: &mut FrameBuffer, horizon: usize, t: f32, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        let water = palette.get(24);
        for y in horizon..h {
            let depth = y - horizon;
            let src_y = horizon.saturating_sub(1 + depth * 3 / 2);
            let shift = (fsin(depth as f32 * 0.9 - t * 5.0) * (1.0 + depth as f32 * 0.12)) as i32;
            self.row.clear();
            self.row.extend_from_slice(fb.row(src_y));
            let dim = (150 - (depth * 160 / (h - horizon).max(1)).min(110)) as u16;
            let out = fb.row_mut(y);
            for (x, px) in out.iter_mut().enumerate() {
                let sx = (x as i32 + shift).clamp(0, w as i32 - 1) as usize;
                *px = self.row[sx].scale(dim).mix(water, 70);
            }
        }
        if horizon < h {
            fb.row_mut(horizon).fill(palette.get(150));
        }
    }
}

impl Effect for Scroller {
    fn name(&self) -> &'static str {
        "scroller"
    }

    fn palette(&self) -> &'static str {
        "copper"
    }

    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette) {
        let (w, h) = (fb.width(), fb.height());
        if w == 0 || h == 0 {
            return;
        }
        let horizon = ((h as f32 * HORIZON) as usize).clamp(1, h);
        let t = frame.time;
        Scroller::copper(fb, horizon, t, palette);
        Scroller::text(fb, horizon, t, palette);
        self.reflect(fb, horizon, t, palette);
    }
}
