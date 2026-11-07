//! 256-entry palettes, the way VGA mode 13h and the Amiga thought about color.
//!
//! Effects never pick colors directly. They compute an 8-bit index (heat,
//! texel, field strength...) and look it up here, which is what makes palette
//! cycling and live palette switching free.
//!
//! Every palette starts at (or very near) black at index 0, so effects that
//! treat the index as intensity, like the fire, work with any of them.

use crate::color::Rgb;

/// A 256-color lookup table.
#[derive(Clone, Debug)]
pub struct Palette {
    name: &'static str,
    colors: [Rgb; 256],
}

impl Palette {
    /// Builds a smooth gradient through `stops`, each `(index, color)`.
    ///
    /// Indices before the first stop take the first color and indices after
    /// the last stop take the last color. Every stop is reproduced exactly.
    ///
    /// # Panics
    /// If `stops` is empty or its indices are not strictly increasing.
    pub fn gradient(name: &'static str, stops: &[(u8, Rgb)]) -> Palette {
        assert!(!stops.is_empty(), "a gradient needs at least one stop");
        assert!(stops.windows(2).all(|w| w[0].0 < w[1].0), "gradient stops must be strictly increasing");
        let mut colors = [stops[0].1; 256];
        for (i, slot) in colors.iter_mut().enumerate() {
            let i = i as u32;
            let seg = stops.windows(2).find(|w| i >= w[0].0 as u32 && i <= w[1].0 as u32);
            *slot = match seg {
                Some(w) => {
                    let (p0, c0) = (w[0].0 as u32, w[0].1);
                    let (p1, c1) = (w[1].0 as u32, w[1].1);
                    let alpha = ((i - p0) * 256 / (p1 - p0)) as u16;
                    c0.mix(c1, alpha)
                }
                None if i < stops[0].0 as u32 => stops[0].1,
                None => stops[stops.len() - 1].1,
            };
        }
        Palette { name, colors }
    }

    /// Spreads `colors` evenly over the 256 entries and interpolates between
    /// them. Handy for porting short hardware ramps like Doom's 37-step fire.
    pub fn ramp(name: &'static str, colors: &[Rgb]) -> Palette {
        assert!(colors.len() >= 2, "a ramp needs at least two colors");
        let last = colors.len() - 1;
        let stops: Vec<(u8, Rgb)> =
            colors.iter().enumerate().map(|(i, &c)| (((i * 255 + last / 2) / last) as u8, c)).collect();
        Palette::gradient(name, &stops)
    }

    /// Hard color bands with no interpolation: each color owns an equal slice
    /// of the index range. This is how 16-color machines did "gradients".
    pub fn banded(name: &'static str, colors: &[Rgb]) -> Palette {
        assert!(!colors.is_empty(), "a banded palette needs at least one color");
        let mut out = [Rgb::BLACK; 256];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = colors[i * colors.len() / 256];
        }
        Palette { name, colors: out }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Direct lookup.
    #[inline]
    pub fn get(&self, index: u8) -> Rgb {
        self.colors[index as usize]
    }

    /// Mirrored lookup for palette cycling.
    ///
    /// The index runs up the palette for 0..128 and back down for 128..256,
    /// so `cyclic(i.wrapping_add(offset))` rotates through the colors with no
    /// seam where 255 wraps to 0, even on a dark-to-bright ramp.
    #[inline]
    pub fn cyclic(&self, index: u8) -> Rgb {
        let tri = if index < 128 { index << 1 } else { (255 - index) << 1 | 1 };
        self.colors[tri as usize]
    }

    pub fn colors(&self) -> &[Rgb; 256] {
        &self.colors
    }
}

/// The 37-entry fire ramp from the PlayStation port of Doom, as documented in
/// Fabien Sanglard's write-up of the effect.
const DOOM_FIRE: [u32; 37] = [
    0x070707, 0x1f0707, 0x2f0f07, 0x470f07, 0x571707, 0x671f07, 0x771f07, 0x8f2707, 0x9f2f07, 0xaf3f07,
    0xbf4707, 0xc74707, 0xdf4f07, 0xdf5707, 0xdf5707, 0xd75f07, 0xd75f07, 0xd7670f, 0xcf6f0f, 0xcf770f,
    0xcf7f0f, 0xcf8717, 0xc78717, 0xc78f17, 0xc7971f, 0xbf9f1f, 0xbf9f1f, 0xbfa727, 0xbfa727, 0xbfaf2f,
    0xb7af2f, 0xb7b72f, 0xb7b737, 0xcfcf6f, 0xdfdf9f, 0xefefc7, 0xffffff,
];

/// Commodore 64 VIC-II colors (Pepto's measured values), the eight that form
/// a clean luminance ladder: black, blue, purple, light blue, cyan, light
/// green, yellow, white.
const C64_LADDER: [u32; 8] = [0x000000, 0x352879, 0x6f3d86, 0x6c5eb5, 0x70a4b2, 0x9ad284, 0xb8c76f, 0xffffff];

fn hexes(v: &[u32]) -> Vec<Rgb> {
    v.iter().map(|&h| Rgb::hex(h)).collect()
}

/// All built-in palettes, in the order the `p` key cycles through them.
pub fn builtin() -> Vec<Palette> {
    vec![
        // Deep aubergine through oxblood to warm cream: the slow, rich plasma
        // look of mid-90s PC intros rather than a hue-wheel rainbow.
        Palette::gradient(
            "velvet",
            &[
                (0, Rgb::hex(0x000000)),
                (40, Rgb::hex(0x1a0b2e)),
                (90, Rgb::hex(0x4b1248)),
                (140, Rgb::hex(0x9c2a4a)),
                (190, Rgb::hex(0xe0703a)),
                (230, Rgb::hex(0xf8c878)),
                (255, Rgb::hex(0xfff4dc)),
            ],
        ),
        Palette::ramp("doom", &hexes(&DOOM_FIRE)),
        // Amiga copper bars: navy into steel blue, a white specular core.
        Palette::gradient(
            "copper",
            &[
                (0, Rgb::hex(0x000008)),
                (60, Rgb::hex(0x0a1a4a)),
                (130, Rgb::hex(0x2a5aa8)),
                (200, Rgb::hex(0x9cc8f0)),
                (255, Rgb::hex(0xffffff)),
            ],
        ),
        // P3 amber phosphor, the monochrome terminal glow.
        Palette::gradient(
            "amber",
            &[
                (0, Rgb::hex(0x000000)),
                (70, Rgb::hex(0x2a1400)),
                (160, Rgb::hex(0xb86e00)),
                (220, Rgb::hex(0xffb000)),
                (255, Rgb::hex(0xffe7b0)),
            ],
        ),
        // Cold liquid metal: near-black indigo up to a hard white highlight.
        Palette::gradient(
            "mercury",
            &[
                (0, Rgb::hex(0x000000)),
                (60, Rgb::hex(0x050a24)),
                (130, Rgb::hex(0x102a6a)),
                (190, Rgb::hex(0x2f6fb8)),
                (235, Rgb::hex(0x8fd0ff)),
                (255, Rgb::hex(0xffffff)),
            ],
        ),
        Palette::banded("c64", &hexes(&C64_LADDER)),
        // P1 green phosphor.
        Palette::gradient(
            "phosphor",
            &[
                (0, Rgb::hex(0x000000)),
                (80, Rgb::hex(0x06280a)),
                (170, Rgb::hex(0x1fa035)),
                (230, Rgb::hex(0x5cff6a)),
                (255, Rgb::hex(0xd8ffd8)),
            ],
        ),
    ]
}

/// Index of the built-in palette called `name`, if any.
pub fn find(palettes: &[Palette], name: &str) -> Option<usize> {
    palettes.iter().position(|p| p.name().eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_reproduces_stops_exactly() {
        let stops = [(0, Rgb::hex(0x000000)), (100, Rgb::hex(0x804020)), (255, Rgb::hex(0xffffff))];
        let p = Palette::gradient("t", &stops);
        for (i, c) in stops {
            assert_eq!(p.get(i), c, "stop at {i}");
        }
    }

    #[test]
    fn gradient_interpolates_linearly() {
        let p = Palette::gradient("t", &[(0, Rgb::BLACK), (255, Rgb::new(255, 0, 0))]);
        for i in 0..=255u8 {
            let r = p.get(i).r as i32;
            assert!((r - i as i32).abs() <= 1, "index {i} gave {r}");
        }
    }

    #[test]
    fn gradient_clamps_outside_stops() {
        let p = Palette::gradient("t", &[(50, Rgb::new(1, 2, 3)), (60, Rgb::new(9, 9, 9))]);
        assert_eq!(p.get(0), Rgb::new(1, 2, 3));
        assert_eq!(p.get(255), Rgb::new(9, 9, 9));
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn gradient_rejects_unsorted_stops() {
        Palette::gradient("t", &[(10, Rgb::BLACK), (5, Rgb::WHITE)]);
    }

    #[test]
    fn ramp_spans_full_range() {
        let p = Palette::ramp("doom", &hexes(&DOOM_FIRE));
        assert_eq!(p.get(0), Rgb::hex(DOOM_FIRE[0]));
        assert_eq!(p.get(255), Rgb::WHITE);
    }

    #[test]
    fn banded_has_equal_hard_bands() {
        let colors = hexes(&C64_LADDER);
        let p = Palette::banded("c64", &colors);
        for (band, &c) in colors.iter().enumerate() {
            for i in band * 32..(band + 1) * 32 {
                assert_eq!(p.get(i as u8), c);
            }
        }
    }

    #[test]
    fn cyclic_is_seamless_and_mirrored() {
        let p = Palette::gradient("t", &[(0, Rgb::BLACK), (255, Rgb::WHITE)]);
        // Neighbouring indices, including the 255 -> 0 wrap, stay close.
        for i in 0..=255u8 {
            let a = p.cyclic(i);
            let b = p.cyclic(i.wrapping_add(1));
            assert!(a.max_delta(b) <= 3, "jump between {i} and {}", i.wrapping_add(1));
        }
        // Up and back down.
        assert_eq!(p.cyclic(0), p.get(0));
        assert_eq!(p.cyclic(127), p.get(254));
        assert_eq!(p.cyclic(128), p.get(255));
        assert_eq!(p.cyclic(255), p.get(1));
    }

    #[test]
    fn builtins_are_unique_and_start_dark() {
        let all = builtin();
        assert!(all.len() >= 6);
        for (i, p) in all.iter().enumerate() {
            assert!(p.get(0).luma() < 16, "{} must start near black", p.name());
            assert!(p.get(255).luma() > 150, "{} must end bright", p.name());
            assert_eq!(find(&all, p.name()), Some(i));
        }
        assert_eq!(find(&all, "DOOM"), Some(1));
        assert_eq!(find(&all, "nope"), None);
    }
}
