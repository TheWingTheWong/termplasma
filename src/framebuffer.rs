//! The pixel canvas every effect draws into.
//!
//! A frame buffer is a plain row-major `Vec<Rgb>`. In the terminal each
//! character cell shows two vertically stacked pixels, so a terminal of
//! `cols x rows` cells is backed by a `cols x (rows * 2)` frame buffer.

use crate::color::Rgb;

#[derive(Clone, Debug, Default)]
pub struct FrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<Rgb>,
}

impl FrameBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        FrameBuffer { width, height, pixels: vec![Rgb::BLACK; width * height] }
    }

    /// Changes the dimensions. Contents are cleared to black when the size
    /// actually changes, and left untouched otherwise.
    pub fn resize(&mut self, width: usize, height: usize) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.pixels.clear();
            self.pixels.resize(width * height, Rgb::BLACK);
        }
    }

    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    pub fn clear(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    /// Pixel at `(x, y)`, or `None` outside the buffer.
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Option<Rgb> {
        (x < self.width && y < self.height).then(|| self.pixels[y * self.width + x])
    }

    /// Writes a pixel; coordinates outside the buffer are ignored, which
    /// keeps effects free of clipping code.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, color: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.pixels[y as usize * self.width + x as usize] = color;
        }
    }

    pub fn pixels(&self) -> &[Rgb] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [Rgb] {
        &mut self.pixels
    }

    /// One row as a mutable slice. Effects use this to write whole
    /// scanlines without per-pixel bounds checks.
    #[inline]
    pub fn row_mut(&mut self, y: usize) -> &mut [Rgb] {
        let w = self.width;
        &mut self.pixels[y * w..(y + 1) * w]
    }

    #[inline]
    pub fn row(&self, y: usize) -> &[Rgb] {
        &self.pixels[y * self.width..(y + 1) * self.width]
    }

    /// Crossfades this buffer toward `other` in place. `alpha` is in
    /// `0..=256` like [`Rgb::mix`]. Buffers must be the same size.
    pub fn blend_toward(&mut self, other: &FrameBuffer, alpha: u16) {
        assert_eq!(
            (self.width, self.height),
            (other.width, other.height),
            "crossfade needs equally sized buffers"
        );
        if alpha == 0 {
            return;
        }
        if alpha >= 256 {
            self.pixels.copy_from_slice(&other.pixels);
            return;
        }
        for (a, &b) in self.pixels.iter_mut().zip(&other.pixels) {
            *a = a.mix(b, alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_clipping() {
        let mut fb = FrameBuffer::new(4, 3);
        fb.set(1, 2, Rgb::WHITE);
        assert_eq!(fb.get(1, 2), Some(Rgb::WHITE));
        assert_eq!(fb.get(4, 0), None);
        fb.set(-1, 0, Rgb::WHITE);
        fb.set(0, 99, Rgb::WHITE);
        assert_eq!(fb.pixels().iter().filter(|&&p| p == Rgb::WHITE).count(), 1);
    }

    #[test]
    fn resize_clears_only_on_change() {
        let mut fb = FrameBuffer::new(2, 2);
        fb.clear(Rgb::WHITE);
        fb.resize(2, 2);
        assert_eq!(fb.get(0, 0), Some(Rgb::WHITE));
        fb.resize(3, 1);
        assert_eq!(fb.pixels().len(), 3);
        assert!(fb.pixels().iter().all(|&p| p == Rgb::BLACK));
    }

    #[test]
    fn blend_endpoints() {
        let mut a = FrameBuffer::new(2, 2);
        let mut b = FrameBuffer::new(2, 2);
        b.clear(Rgb::new(200, 100, 0));
        a.blend_toward(&b, 0);
        assert_eq!(a.get(0, 0), Some(Rgb::BLACK));
        a.blend_toward(&b, 128);
        assert_eq!(a.get(1, 1), Some(Rgb::new(100, 50, 0)));
        a.blend_toward(&b, 256);
        assert_eq!(a.pixels(), b.pixels());
    }

    #[test]
    fn zero_sized_buffers_are_fine() {
        let mut fb = FrameBuffer::new(0, 0);
        assert!(fb.is_empty());
        fb.clear(Rgb::WHITE);
        fb.set(0, 0, Rgb::WHITE);
        assert_eq!(fb.get(0, 0), None);
    }
}
