//! Rendering without a terminal: the full pipeline (effects, crossfades,
//! cell composition, diffing and ANSI encoding) into memory, with a fixed
//! time step so runs are reproducible. Used for benchmarks and tests.

use std::time::{Duration, Instant};

use crate::demo::Demo;
use crate::framebuffer::FrameBuffer;
use crate::render::Renderer;

/// What a headless run cost.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub frames: u64,
    pub cols: usize,
    pub rows: usize,
    /// Time spent inside effects (including crossfades).
    pub effect_time: Duration,
    /// Time spent composing cells, diffing and encoding.
    pub encode_time: Duration,
    pub bytes: u64,
    pub cells_changed: u64,
}

impl Report {
    pub fn total_time(&self) -> Duration {
        self.effect_time + self.encode_time
    }

    fn per_frame(&self, d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0 / self.frames.max(1) as f64
    }

    pub fn effect_ms(&self) -> f64 {
        self.per_frame(self.effect_time)
    }

    pub fn encode_ms(&self) -> f64 {
        self.per_frame(self.encode_time)
    }

    /// Frames per second the pipeline could sustain, ignoring terminal I/O.
    pub fn fps(&self) -> f64 {
        self.frames as f64 / self.total_time().as_secs_f64().max(1e-9)
    }

    pub fn bytes_per_frame(&self) -> f64 {
        self.bytes as f64 / self.frames.max(1) as f64
    }

    /// Fraction of cells re-sent per frame, 0..=1.
    pub fn change_ratio(&self) -> f64 {
        self.cells_changed as f64 / (self.frames.max(1) as f64 * (self.cols * self.rows).max(1) as f64)
    }
}

/// Renders `frames` frames of `demo` at `cols x rows` cells, stepping time
/// by `dt` seconds per frame. Returns the report and the final frame.
pub fn run(demo: &mut Demo, renderer: &mut Renderer, frames: u64, dt: f32) -> (Report, FrameBuffer) {
    let (cols, rows) = (renderer.cols(), renderer.rows());
    let mut fb = FrameBuffer::new(cols, rows * 2);
    let mut report = Report { frames, cols, rows, ..Report::default() };
    for _ in 0..frames {
        let t0 = Instant::now();
        demo.update(dt);
        demo.render(&mut fb);
        let t1 = Instant::now();
        let stats = renderer.render(&fb, &[]);
        let t2 = Instant::now();
        report.effect_time += t1 - t0;
        report.encode_time += t2 - t1;
        report.bytes += stats.bytes as u64;
        report.cells_changed += stats.cells_changed as u64;
    }
    (report, fb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_run_counts_everything() {
        let mut demo = Demo::standard();
        let mut r = Renderer::new(40, 12);
        let (report, fb) = run(&mut demo, &mut r, 30, 1.0 / 60.0);
        assert_eq!(report.frames, 30);
        assert_eq!((fb.width(), fb.height()), (40, 24));
        assert!(report.bytes > 0);
        assert!(report.change_ratio() > 0.0 && report.change_ratio() <= 1.0);
        assert!(report.fps() > 0.0);
    }

    #[test]
    fn headless_runs_are_deterministic() {
        let render = || {
            let mut demo = Demo::standard();
            assert!(demo.select_name("fire"));
            let mut r = Renderer::new(30, 10);
            let (report, fb) = run(&mut demo, &mut r, 20, 1.0 / 60.0);
            (report.bytes, fb.pixels().to_vec())
        };
        assert_eq!(render(), render());
    }
}
