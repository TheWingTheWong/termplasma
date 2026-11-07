//! Property test for the diffing encoder: replaying the byte stream on a
//! minimal terminal emulator must reproduce every frame exactly.
//!
//! This catches the bugs that unit tests on exact byte strings miss: a
//! skipped cell, a stale color carried across a cursor jump, an off-by-one
//! in CUF, a wrong row after writing the last column.

use termplasma::color::Rgb;
use termplasma::framebuffer::FrameBuffer;
use termplasma::render::{Cell, ColorMode, Overlay, Renderer};
use termplasma::rng::Rng;

/// Just enough of a VT100/xterm to understand what the renderer emits.
struct Vt {
    cols: usize,
    rows: usize,
    grid: Vec<Option<Cell>>,
    x: usize,
    y: usize,
    fg: Rgb,
    bg: Rgb,
}

impl Vt {
    fn new(cols: usize, rows: usize) -> Self {
        Vt { cols, rows, grid: vec![None; cols * rows], x: 0, y: 0, fg: Rgb::BLACK, bg: Rgb::BLACK }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let s = std::str::from_utf8(bytes).expect("renderer output is valid UTF-8");
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                assert!(self.x < self.cols && self.y < self.rows, "write outside the screen");
                self.grid[self.y * self.cols + self.x] = Some(Cell { ch: c, fg: self.fg, bg: self.bg });
                self.x += 1;
                continue;
            }
            assert_eq!(chars.next(), Some('['), "only CSI sequences are expected");
            let mut params = String::new();
            let fin = loop {
                let c = chars.next().expect("unterminated CSI");
                if c.is_ascii_digit() || c == ';' || c == '?' {
                    params.push(c);
                } else {
                    break c;
                }
            };
            let nums: Vec<usize> =
                params.trim_start_matches('?').split(';').map(|p| p.parse().unwrap_or(0)).collect();
            match fin {
                'H' => {
                    self.y = nums[0] - 1;
                    self.x = nums[1] - 1;
                }
                'C' => self.x += nums[0].max(1),
                'm' => self.sgr(&nums),
                'h' | 'l' => assert_eq!(params, "?2026", "unexpected mode toggle"),
                other => panic!("unexpected CSI final byte {other:?}"),
            }
        }
    }

    fn sgr(&mut self, p: &[usize]) {
        let mut i = 0;
        while i < p.len() {
            let target = p[i];
            let color = match p[i + 1] {
                2 => {
                    let c = Rgb::new(p[i + 2] as u8, p[i + 3] as u8, p[i + 4] as u8);
                    i += 5;
                    c
                }
                5 => {
                    let c = Rgb::from_ansi256(p[i + 2] as u8);
                    i += 3;
                    c
                }
                other => panic!("unexpected color kind {other}"),
            };
            match target {
                38 => self.fg = color,
                48 => self.bg = color,
                other => panic!("unexpected SGR {other}"),
            }
        }
    }

    /// Compares a screen cell to the intended one, ignoring the invisible
    /// foreground of space cells.
    fn assert_matches(&self, r: &Renderer, frame: usize) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let want = r.cell(col, row);
                let got = self.grid[row * self.cols + col]
                    .unwrap_or_else(|| panic!("frame {frame}: cell ({col},{row}) never drawn"));
                assert_eq!(got.ch, want.ch, "frame {frame}: glyph at ({col},{row})");
                assert_eq!(got.bg, want.bg, "frame {frame}: background at ({col},{row})");
                if want.ch != ' ' {
                    assert_eq!(got.fg, want.fg, "frame {frame}: foreground at ({col},{row})");
                }
            }
        }
    }
}

/// Mutates a frame buffer the way animations do: some regions change, most
/// stay put, and colors repeat often enough to exercise SGR reuse.
fn scribble(fb: &mut FrameBuffer, rng: &mut Rng) {
    let palette = [Rgb::BLACK, Rgb::WHITE, Rgb::new(200, 30, 30), Rgb::new(10, 80, 160)];
    let n = rng.below((fb.width() * fb.height()) as u32 / 3 + 1);
    for _ in 0..n {
        let x = rng.below(fb.width() as u32) as i32;
        let y = rng.below(fb.height() as u32) as i32;
        let c = if rng.below(4) == 0 {
            Rgb::new(rng.next_u32() as u8, rng.next_u32() as u8, rng.next_u32() as u8)
        } else {
            palette[rng.below(4) as usize]
        };
        fb.set(x, y, c);
    }
}

fn run_replay(cols: usize, rows: usize, mode: ColorMode, seed: u32) {
    let mut rng = Rng::new(seed);
    let mut r = Renderer::new(cols, rows);
    r.set_color_mode(mode);
    let mut vt = Vt::new(cols, rows);
    let mut fb = FrameBuffer::new(cols, rows * 2);
    for frame in 0..60 {
        scribble(&mut fb, &mut rng);
        let hud = format!("frame {frame}");
        let overlays =
            [Overlay { col: frame % cols, row: frame % rows, text: &hud, fg: Rgb::WHITE, bg: None }];
        let used = if frame % 3 == 0 { &overlays[..] } else { &[] };
        r.render(&fb, used);
        vt.feed(r.output());
        vt.assert_matches(&r, frame);
    }
}

#[test]
fn replay_reproduces_every_frame_truecolor() {
    for (i, &(c, r)) in [(1, 1), (2, 3), (17, 5), (80, 24), (133, 41)].iter().enumerate() {
        run_replay(c, r, ColorMode::TrueColor, 1000 + i as u32);
    }
}

#[test]
fn replay_reproduces_every_frame_256_colors() {
    run_replay(40, 12, ColorMode::Ansi256, 77);
}

#[test]
fn replay_survives_resize() {
    let mut r = Renderer::new(10, 4);
    let mut fb = FrameBuffer::new(10, 8);
    let mut rng = Rng::new(5);
    scribble(&mut fb, &mut rng);
    r.render(&fb, &[]);

    // After a resize the terminal content is undefined; a fresh emulator
    // stands in for that and must still end up fully correct.
    r.resize(7, 6);
    fb.resize(7, 12);
    scribble(&mut fb, &mut rng);
    r.render(&fb, &[]);
    let mut vt = Vt::new(7, 6);
    vt.feed(r.output());
    vt.assert_matches(&r, 0);
}
