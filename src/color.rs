//! 24-bit RGB colors and the small amount of integer color math the
//! renderer and effects need.

/// A 24-bit truecolor value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    #[inline]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Builds a color from a `0xRRGGBB` literal.
    #[inline]
    pub const fn hex(v: u32) -> Self {
        Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Linear blend from `self` to `other`.
    ///
    /// `alpha` is fixed point in `0..=256`: 0 returns `self` exactly and 256
    /// returns `other` exactly, so a full fade never leaves a residue.
    #[inline]
    pub fn mix(self, other: Rgb, alpha: u16) -> Rgb {
        #[inline]
        fn ch(a: u8, b: u8, alpha: i32) -> u8 {
            let a = a as i32;
            (a + (((b as i32 - a) * alpha) >> 8)) as u8
        }
        let alpha = alpha.min(256) as i32;
        Rgb::new(ch(self.r, other.r, alpha), ch(self.g, other.g, alpha), ch(self.b, other.b, alpha))
    }

    /// Multiplies every channel by `s / 256` (`s` in `0..=256`).
    #[inline]
    pub fn scale(self, s: u16) -> Rgb {
        let s = s.min(256) as u32;
        Rgb::new(
            ((self.r as u32 * s) >> 8) as u8,
            ((self.g as u32 * s) >> 8) as u8,
            ((self.b as u32 * s) >> 8) as u8,
        )
    }

    /// Per-channel saturating addition, used for additive glows.
    #[inline]
    pub fn saturating_add(self, other: Rgb) -> Rgb {
        Rgb::new(
            self.r.saturating_add(other.r),
            self.g.saturating_add(other.g),
            self.b.saturating_add(other.b),
        )
    }

    /// Integer Rec.601 luma in `0..=255`.
    #[inline]
    pub fn luma(self) -> u8 {
        ((77 * self.r as u32 + 150 * self.g as u32 + 29 * self.b as u32) >> 8) as u8
    }

    /// Largest absolute difference across the three channels.
    #[inline]
    pub fn max_delta(self, other: Rgb) -> u8 {
        self.r.abs_diff(other.r).max(self.g.abs_diff(other.g)).max(self.b.abs_diff(other.b))
    }

    /// Average of two colors, rounding down.
    #[inline]
    pub fn average(self, other: Rgb) -> Rgb {
        Rgb::new(
            ((self.r as u16 + other.r as u16) >> 1) as u8,
            ((self.g as u16 + other.g as u16) >> 1) as u8,
            ((self.b as u16 + other.b as u16) >> 1) as u8,
        )
    }

    /// Nearest entry of the xterm 256-color palette (the 6x6x6 cube plus the
    /// 24-step gray ramp). Used when the terminal does not speak truecolor.
    pub fn to_ansi256(self) -> u8 {
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        fn cube_index(v: u8) -> usize {
            if v < 48 {
                0
            } else if v < 115 {
                1
            } else {
                ((v as usize - 35) / 40).min(5)
            }
        }
        let (ri, gi, bi) = (cube_index(self.r), cube_index(self.g), cube_index(self.b));
        let cube = Rgb::new(LEVELS[ri], LEVELS[gi], LEVELS[bi]);

        let avg = (self.r as u32 + self.g as u32 + self.b as u32) / 3;
        let gray_i = if avg > 238 { 23 } else { (avg.saturating_sub(3) / 10).min(23) };
        let gray_v = (8 + gray_i * 10) as u8;
        let gray = Rgb::new(gray_v, gray_v, gray_v);

        if gray.dist2(self) < cube.dist2(self) {
            232 + gray_i as u8
        } else {
            (16 + 36 * ri + 6 * gi + bi) as u8
        }
    }

    /// The color an xterm-256 index actually displays (inverse of
    /// [`Rgb::to_ansi256`] for indices 16..=255).
    pub fn from_ansi256(i: u8) -> Rgb {
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        match i {
            16..=231 => {
                let i = i as usize - 16;
                Rgb::new(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
            }
            232..=255 => {
                let v = 8 + (i - 232) * 10;
                Rgb::new(v, v, v)
            }
            _ => Rgb::BLACK,
        }
    }

    #[inline]
    fn dist2(self, other: Rgb) -> u32 {
        let dr = self.r as i32 - other.r as i32;
        let dg = self.g as i32 - other.g as i32;
        let db = self.b as i32 - other.b as i32;
        (dr * dr + dg * dg + db * db) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_hits_both_endpoints_exactly() {
        let a = Rgb::hex(0x102030);
        let b = Rgb::hex(0xf0e0d0);
        assert_eq!(a.mix(b, 0), a);
        assert_eq!(a.mix(b, 256), b);
        assert_eq!(b.mix(a, 256), a);
    }

    #[test]
    fn mix_midpoint_and_monotonic() {
        let a = Rgb::BLACK;
        let b = Rgb::new(200, 100, 50);
        assert_eq!(a.mix(b, 128), Rgb::new(100, 50, 25));
        let mut prev = 0;
        for alpha in 0..=256 {
            let c = a.mix(b, alpha);
            assert!(c.r >= prev, "mix must be monotonic in alpha");
            prev = c.r;
        }
    }

    #[test]
    fn mix_clamps_alpha() {
        let a = Rgb::BLACK;
        let b = Rgb::WHITE;
        assert_eq!(a.mix(b, 1000), b);
    }

    #[test]
    fn scale_and_add() {
        assert_eq!(Rgb::WHITE.scale(256), Rgb::WHITE);
        assert_eq!(Rgb::WHITE.scale(0), Rgb::BLACK);
        assert_eq!(Rgb::new(200, 10, 0).saturating_add(Rgb::new(100, 10, 0)), Rgb::new(255, 20, 0));
    }

    #[test]
    fn luma_extremes() {
        assert_eq!(Rgb::BLACK.luma(), 0);
        assert_eq!(Rgb::WHITE.luma(), 255);
        assert!(Rgb::new(0, 255, 0).luma() > Rgb::new(255, 0, 0).luma());
    }

    #[test]
    fn hex_and_delta() {
        assert_eq!(Rgb::hex(0x123456), Rgb::new(0x12, 0x34, 0x56));
        assert_eq!(Rgb::new(10, 50, 0).max_delta(Rgb::new(12, 40, 3)), 10);
    }

    #[test]
    fn ansi256_round_trips_palette_entries() {
        for i in 16..=255u8 {
            let c = Rgb::from_ansi256(i);
            assert_eq!(Rgb::from_ansi256(c.to_ansi256()), c, "index {i}");
        }
        assert_eq!(Rgb::BLACK.to_ansi256(), 16);
        assert_eq!(Rgb::WHITE.to_ansi256(), 231);
    }
}
