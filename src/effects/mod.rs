//! The effects. Each one owns whatever tables and simulation state it needs
//! and draws a complete frame into a [`FrameBuffer`] on every call.
//!
//! Effects size themselves lazily from the frame buffer they are handed, so
//! there is no separate resize hook to forget: when the dimensions change,
//! the next `render` rebuilds the tables.

use crate::framebuffer::FrameBuffer;
use crate::palette::Palette;

pub mod plasma;

/// Timing for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// Seconds of demo time (scaled by playback speed, frozen while paused).
    pub time: f32,
    /// Demo-time seconds since the previous frame; 0 while paused.
    pub dt: f32,
}

pub trait Effect {
    /// Short lowercase identifier, used on the command line.
    fn name(&self) -> &'static str;

    /// The palette this effect is designed around.
    fn palette(&self) -> &'static str;

    /// Draws a full frame. Must cope with any size, including zero.
    fn render(&mut self, fb: &mut FrameBuffer, frame: Frame, palette: &Palette);
}

/// One instance of every effect, in demo order.
pub fn all() -> Vec<Box<dyn Effect>> {
    vec![Box::new(plasma::Plasma::default())]
}
