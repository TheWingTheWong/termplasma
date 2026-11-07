//! termplasma: classic demoscene effects rendered live in the terminal.
//!
//! The crate is split into a small, terminal-agnostic core (colors, palettes,
//! lookup tables, the frame buffer and the diffing ANSI encoder) and a set of
//! effects that draw into that frame buffer. The binary wires the core to a
//! real terminal through crossterm, or runs it headless for benchmarks.

pub mod app;
pub mod cli;
pub mod color;
pub mod demo;
pub mod effects;
pub mod font;
pub mod framebuffer;
pub mod headless;
pub mod hud;
pub mod lut;
pub mod palette;
pub mod preview;
pub mod render;
pub mod rng;
