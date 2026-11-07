//! Plain-text previews of a frame, for READMEs, logs and terminals that
//! cannot show color at all.

use crate::framebuffer::FrameBuffer;

/// Density ramp from empty to solid.
const RAMP: &[u8] = b" .,:;-=+*#%@";

/// Renders `fb` as ASCII art, one character per terminal cell (each covering
/// two vertically stacked pixels, the same geometry as the live renderer).
pub fn to_ascii(fb: &FrameBuffer) -> String {
    let (w, h) = (fb.width(), fb.height());
    let mut out = String::with_capacity((w + 1) * h.div_ceil(2));
    for row in 0..h.div_ceil(2) {
        for x in 0..w {
            let top = fb.get(x, row * 2).map_or(0, |c| c.luma() as u32);
            let bottom = fb.get(x, row * 2 + 1).map_or(top, |c| c.luma() as u32);
            let l = (top + bottom) / 2;
            out.push(RAMP[(l as usize * RAMP.len()) / 256] as char);
        }
        // Trailing spaces only make diffs and copy-paste worse.
        let trimmed = out.trim_end_matches(' ').len();
        out.truncate(trimmed);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    #[test]
    fn ramp_extremes_and_shape() {
        let mut fb = FrameBuffer::new(3, 4);
        fb.set(0, 0, Rgb::WHITE);
        fb.set(0, 1, Rgb::WHITE);
        let s = to_ascii(&fb);
        assert_eq!(s, "@\n\n");
    }

    #[test]
    fn odd_height_uses_last_row() {
        let mut fb = FrameBuffer::new(1, 1);
        fb.clear(Rgb::WHITE);
        assert_eq!(to_ascii(&fb), "@\n");
    }
}
