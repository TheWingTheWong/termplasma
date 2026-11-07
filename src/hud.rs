//! The status line, the help panel and the frame-rate meter.

use std::time::{Duration, Instant};

use crate::color::Rgb;
use crate::demo::Demo;
use crate::render::{FrameStats, Overlay};

const HUD_FG: Rgb = Rgb::hex(0xd8d4c8);
const HELP_FG: Rgb = Rgb::hex(0xe8e4d8);
const HELP_BG: Rgb = Rgb::hex(0x0c0b10);

/// Frames per second and bytes per frame, averaged over half-second windows
/// so the readout is stable enough to read.
#[derive(Debug)]
pub struct Meter {
    window_start: Option<Instant>,
    frames: u32,
    bytes: usize,
    fps: f32,
    bytes_per_frame: f32,
}

impl Default for Meter {
    fn default() -> Self {
        Meter { window_start: None, frames: 0, bytes: 0, fps: 0.0, bytes_per_frame: 0.0 }
    }
}

impl Meter {
    const WINDOW: Duration = Duration::from_millis(500);

    pub fn tick(&mut self, now: Instant, stats: FrameStats) {
        let start = *self.window_start.get_or_insert(now);
        self.frames += 1;
        self.bytes += stats.bytes;
        let elapsed = now.duration_since(start);
        if elapsed >= Self::WINDOW {
            let secs = elapsed.as_secs_f32();
            self.fps = self.frames as f32 / secs;
            self.bytes_per_frame = self.bytes as f32 / self.frames as f32;
            self.window_start = Some(now);
            self.frames = 0;
            self.bytes = 0;
        }
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }

    pub fn bytes_per_frame(&self) -> f32 {
        self.bytes_per_frame
    }
}

/// Human-sized byte counts: `812 B`, `14.2 KB`.
pub fn format_bytes(b: f32) -> String {
    if b < 1000.0 { format!("{b:.0} B") } else { format!("{:.1} KB", b / 1024.0) }
}

/// The one-line status text.
pub fn status_text(demo: &Demo, meter: &Meter) -> String {
    let mut s = format!(
        " {}/{} {}  {}{}  {:.0} fps  {}/frame",
        demo.effect_index() + 1,
        demo.effect_count(),
        demo.effect_name(),
        demo.palette_name(),
        if demo.palette_is_override() { "*" } else { "" },
        meter.fps(),
        format_bytes(meter.bytes_per_frame()),
    );
    if demo.speed() != 1.0 {
        s.push_str(&format!("  x{}", demo.speed()));
    }
    s.push_str(if demo.is_paused() {
        "  paused"
    } else if demo.is_autoplay() {
        "  autoplay"
    } else {
        "  manual"
    });
    s.push_str("  h help ");
    s
}

/// Overlay for the status line on the bottom row.
pub fn status_overlay(text: &str, rows: usize) -> Overlay<'_> {
    Overlay { col: 0, row: rows.saturating_sub(1), text, fg: HUD_FG, bg: None }
}

pub const HELP_LINES: &[&str] = &[
    "",
    "  termplasma",
    "",
    "  n  right     next effect",
    "  b  left      previous effect",
    "  1 - 9        jump to effect",
    "  p  P         next / previous palette",
    "  0            each effect's own palette",
    "  space        pause",
    "  a            autoplay on / off",
    "  [  ]         slower / faster",
    "  f            status line on / off",
    "  h  ?         close this help",
    "  q  esc       quit",
    "",
];

/// Width of the help panel, in cells.
pub fn help_width() -> usize {
    HELP_LINES.iter().map(|l| l.chars().count()).max().unwrap_or(0) + 3
}

/// Pads the help lines to a uniform width so the panel is a solid block.
pub fn help_text() -> Vec<String> {
    let w = help_width();
    HELP_LINES.iter().map(|l| format!("{l:<w$}")).collect()
}

/// Overlays for the help panel, centered on a `cols x rows` screen.
pub fn help_overlays(lines: &[String], cols: usize, rows: usize) -> Vec<Overlay<'_>> {
    let w = help_width();
    let col = cols.saturating_sub(w) / 2;
    let row = rows.saturating_sub(lines.len()) / 2;
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| Overlay { col, row: row + i, text: l, fg: HELP_FG, bg: Some(HELP_BG) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_averages_over_window() {
        let mut m = Meter::default();
        let t0 = Instant::now();
        let stats = FrameStats { cells_changed: 1, bytes: 2048 };
        for i in 0..=30 {
            m.tick(t0 + Duration::from_millis(i * 1000 / 60), stats);
        }
        assert!((m.fps() - 60.0).abs() < 3.0, "fps {}", m.fps());
        assert!((m.bytes_per_frame() - 2048.0).abs() < 1.0);
    }

    #[test]
    fn bytes_format() {
        assert_eq!(format_bytes(812.0), "812 B");
        assert_eq!(format_bytes(14540.8), "14.2 KB");
    }

    #[test]
    fn status_reflects_demo_state() {
        let mut d = Demo::standard();
        let m = Meter::default();
        let s = status_text(&d, &m);
        assert!(s.starts_with(" 1/7 starfield  copper  "), "{s}");
        assert!(s.contains("autoplay"));
        d.toggle_pause();
        d.next_palette();
        d.faster();
        let s = status_text(&d, &m);
        assert!(s.contains("amber*"), "{s}");
        assert!(s.contains("x1.5") && s.contains("paused"), "{s}");
    }

    #[test]
    fn help_is_a_centered_block() {
        let lines = help_text();
        let w = help_width();
        assert!(lines.iter().all(|l| l.chars().count() == w));
        let o = help_overlays(&lines, 100, 40);
        assert_eq!(o.len(), lines.len());
        assert_eq!(o[0].col, (100 - w) / 2);
        // Smaller than the panel: no underflow.
        let o = help_overlays(&lines, 10, 3);
        assert_eq!(o[0].col, 0);
    }
}
