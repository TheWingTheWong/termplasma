//! Offline previews of a frame: ASCII art for READMEs and logs, and binary
//! PPM images for pixel-exact screenshots without a screen recorder.

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

/// Encodes `fb` as a binary PPM (P6) image, upscaled by an integer `scale`
/// so the chunky pixels stay sharp in any image viewer.
pub fn to_ppm(fb: &FrameBuffer, scale: usize) -> Vec<u8> {
    let scale = scale.max(1);
    let (w, h) = (fb.width() * scale, fb.height() * scale);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.reserve(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let c = fb.get(x / scale, y / scale).unwrap_or_default();
            out.extend_from_slice(&[c.r, c.g, c.b]);
        }
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
    fn ppm_header_and_scaling() {
        let mut fb = FrameBuffer::new(2, 1);
        fb.set(1, 0, Rgb::new(10, 20, 30));
        let ppm = to_ppm(&fb, 2);
        let header = b"P6\n4 2\n255\n";
        assert_eq!(&ppm[..header.len()], header);
        let px = &ppm[header.len()..];
        assert_eq!(px.len(), 4 * 2 * 3);
        assert_eq!(&px[6..9], &[10, 20, 30], "third pixel of row 0 is the upscaled one");
        assert_eq!(&px[0..3], &[0, 0, 0]);
    }

    #[test]
    fn odd_height_uses_last_row() {
        let mut fb = FrameBuffer::new(1, 1);
        fb.clear(Rgb::WHITE);
        assert_eq!(to_ascii(&fb), "@\n");
    }
}
