//! Frame buffer to terminal: half-block cells, diffing and ANSI encoding.
//!
//! Each terminal cell shows two pixels with `▀` (U+2580, UPPER HALF BLOCK):
//! the foreground color paints the top half and the background color paints
//! the bottom half. When both pixels match, a plain space with only a
//! background color is cheaper and looks identical.
//!
//! The renderer keeps a copy of what the terminal currently shows (`front`)
//! and, each frame, emits escape sequences only for cells that differ from
//! the new frame (`back`). Within a frame it tracks the cursor and current
//! colors so contiguous runs need no cursor moves and repeated colors need no
//! SGR codes. The whole frame is assembled in one buffer and written with a
//! single syscall, wrapped in synchronized-update markers (DEC mode 2026) so
//! terminals that support it never show a half-drawn frame.

use crate::color::Rgb;
use crate::framebuffer::FrameBuffer;

/// The upper half block: foreground = top pixel, background = bottom pixel.
pub const HALF_BLOCK: char = '\u{2580}';
/// The lower half block: the same picture as [`HALF_BLOCK`] with the two
/// colors swapped between foreground and background.
pub const LOWER_HALF_BLOCK: char = '\u{2584}';
/// A full block shows only the foreground color.
pub const FULL_BLOCK: char = '\u{2588}';

/// How colors are sent to the terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit `38;2;r;g;b` sequences.
    TrueColor,
    /// xterm 256-color `38;5;n` sequences, for terminals without truecolor.
    Ansi256,
}

/// One terminal cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

impl Cell {
    /// A cell showing two stacked pixels, normalised so that a uniform cell
    /// is always a space (and its unused foreground is pinned to the
    /// background so equal-looking cells compare equal).
    #[inline]
    pub fn pixels(top: Rgb, bottom: Rgb) -> Cell {
        if top == bottom {
            Cell { ch: ' ', fg: bottom, bg: bottom }
        } else {
            Cell { ch: HALF_BLOCK, fg: top, bg: bottom }
        }
    }

    /// Whether the foreground color is visible at all.
    #[inline]
    fn uses_fg(&self) -> bool {
        self.ch != ' '
    }

    /// True when the two cells look the same within `tolerance` per channel.
    #[inline]
    fn looks_like(&self, other: &Cell, tolerance: u8) -> bool {
        if self.ch != other.ch || self.bg.max_delta(other.bg) > tolerance {
            return false;
        }
        !self.uses_fg() || self.fg.max_delta(other.fg) <= tolerance
    }
}

/// A run of text drawn on top of the pixels (HUD, help panel).
#[derive(Clone, Debug)]
pub struct Overlay<'a> {
    pub col: usize,
    pub row: usize,
    pub text: &'a str,
    pub fg: Rgb,
    /// Solid background, or `None` to darken whatever pixels are underneath.
    pub bg: Option<Rgb>,
}

/// What one frame cost on the wire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub cells_changed: usize,
    pub bytes: usize,
}

pub struct Renderer {
    cols: usize,
    rows: usize,
    front: Vec<Cell>,
    back: Vec<Cell>,
    front_valid: bool,
    out: Vec<u8>,
    tolerance: u8,
    synchronized: bool,
    color_mode: ColorMode,
}

impl Renderer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let blank = Cell::pixels(Rgb::BLACK, Rgb::BLACK);
        Renderer {
            cols,
            rows,
            front: vec![blank; cols * rows],
            back: vec![blank; cols * rows],
            front_valid: false,
            out: Vec::with_capacity(cols * rows * 24),
            tolerance: 0,
            synchronized: true,
            color_mode: ColorMode::TrueColor,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Resizes the cell grid. The next frame is a full redraw.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            let blank = Cell::pixels(Rgb::BLACK, Rgb::BLACK);
            self.front = vec![blank; cols * rows];
            self.back = vec![blank; cols * rows];
        }
        self.invalidate();
    }

    /// Forgets what the terminal shows; the next frame redraws every cell.
    pub fn invalidate(&mut self) {
        self.front_valid = false;
    }

    /// Lossy diffing: a cell is left alone when every channel is within
    /// `tolerance` of what is already on screen. Because the comparison is
    /// against the screen, not the previous frame, error cannot accumulate.
    pub fn set_tolerance(&mut self, tolerance: u8) {
        self.tolerance = tolerance;
    }

    /// Toggles the DEC 2026 synchronized-update wrapper.
    pub fn set_synchronized(&mut self, on: bool) {
        self.synchronized = on;
    }

    pub fn set_color_mode(&mut self, mode: ColorMode) {
        if mode != self.color_mode {
            self.color_mode = mode;
            self.invalidate();
        }
    }

    pub fn color_mode(&self) -> ColorMode {
        self.color_mode
    }

    /// The cell the most recent [`Renderer::compose`] produced.
    pub fn cell(&self, col: usize, row: usize) -> Cell {
        self.back[row * self.cols + col]
    }

    /// Bytes produced by the last [`Renderer::encode`].
    pub fn output(&self) -> &[u8] {
        &self.out
    }

    /// Composes and encodes a frame in one go.
    pub fn render(&mut self, fb: &FrameBuffer, overlays: &[Overlay]) -> FrameStats {
        self.compose(fb, overlays);
        self.encode()
    }

    /// Turns pixel pairs into cells and stamps overlays on top.
    ///
    /// Pixel row `2r` becomes the top half of cell row `r`, pixel row
    /// `2r + 1` the bottom half. Pixels missing from a smaller buffer read
    /// as black, so a stale buffer during a resize cannot panic.
    pub fn compose(&mut self, fb: &FrameBuffer, overlays: &[Overlay]) {
        let quantize = self.color_mode == ColorMode::Ansi256;
        let q = |c: Rgb| if quantize { Rgb::from_ansi256(c.to_ansi256()) } else { c };
        let full = fb.width() >= self.cols && fb.height() >= self.rows * 2;

        for row in 0..self.rows {
            let cells = &mut self.back[row * self.cols..(row + 1) * self.cols];
            if full && !quantize {
                let top = &fb.row(row * 2)[..self.cols];
                let bottom = &fb.row(row * 2 + 1)[..self.cols];
                for ((cell, &t), &b) in cells.iter_mut().zip(top).zip(bottom) {
                    *cell = Cell::pixels(t, b);
                }
            } else {
                for (col, cell) in cells.iter_mut().enumerate() {
                    let t = fb.get(col, row * 2).unwrap_or(Rgb::BLACK);
                    let b = fb.get(col, row * 2 + 1).unwrap_or(Rgb::BLACK);
                    *cell = Cell::pixels(q(t), q(b));
                }
            }
        }

        for o in overlays {
            if o.row >= self.rows {
                continue;
            }
            for (i, ch) in o.text.chars().enumerate() {
                let col = o.col + i;
                if col >= self.cols {
                    break;
                }
                let cell = &mut self.back[o.row * self.cols + col];
                let under = cell.fg.average(cell.bg);
                let bg = o.bg.unwrap_or_else(|| under.scale(72));
                *cell = Cell { ch, fg: q(o.fg), bg: q(bg) };
            }
        }
    }

    /// Diffs the composed frame against the screen and encodes the changes.
    pub fn encode(&mut self) -> FrameStats {
        self.out.clear();
        if self.synchronized {
            self.out.extend_from_slice(b"\x1b[?2026h");
        }
        let prefix_len = self.out.len();

        let mut cursor: Option<(usize, usize)> = None;
        let mut cur_fg: Option<Rgb> = None;
        let mut cur_bg: Option<Rgb> = None;
        let mut changed = 0;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let i = row * self.cols + col;
                let cell = self.back[i];
                if self.front_valid && self.front[i].looks_like(&cell, self.tolerance) {
                    continue;
                }

                // Position the cursor: nothing for a contiguous run, a short
                // forward jump on the same row, a full move otherwise.
                match cursor {
                    Some((c, r)) if r == row && c == col => {}
                    Some((c, r)) if r == row && c < col => {
                        self.out.extend_from_slice(b"\x1b[");
                        push_num(&mut self.out, col - c);
                        self.out.push(b'C');
                    }
                    _ => {
                        self.out.extend_from_slice(b"\x1b[");
                        push_num(&mut self.out, row + 1);
                        self.out.push(b';');
                        push_num(&mut self.out, col + 1);
                        self.out.push(b'H');
                    }
                }

                let (ch, fg, bg) = pick_glyph(&cell, cur_fg, cur_bg);
                let need_fg = ch != ' ' && cur_fg != Some(fg);
                let need_bg = ch != FULL_BLOCK && cur_bg != Some(bg);
                if need_fg || need_bg {
                    self.out.extend_from_slice(b"\x1b[");
                    if need_fg {
                        push_color(&mut self.out, self.color_mode, 38, fg);
                        cur_fg = Some(fg);
                    }
                    if need_bg {
                        if need_fg {
                            self.out.push(b';');
                        }
                        push_color(&mut self.out, self.color_mode, 48, bg);
                        cur_bg = Some(bg);
                    }
                    self.out.push(b'm');
                }

                let mut utf8 = [0u8; 4];
                self.out.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
                self.front[i] = cell;
                changed += 1;

                // Writing the last column leaves the cursor in the terminal's
                // pending-wrap state; force an explicit move afterwards.
                cursor = (col + 1 < self.cols).then_some((col + 1, row));
            }
        }

        if changed == 0 {
            self.out.clear();
        } else if self.synchronized {
            debug_assert!(self.out.len() > prefix_len);
            self.out.extend_from_slice(b"\x1b[?2026l");
        }
        self.front_valid = true;
        FrameStats { cells_changed: changed, bytes: self.out.len() }
    }
}

/// Chooses how to draw a cell given the colors the terminal already has
/// selected, to avoid SGR sequences, which cost 10 to 19 bytes each.
///
/// A two-color cell can be drawn as `▀` (fg = top) or as `▄` (fg = bottom);
/// a one-color cell can be a space (bg) or a full block (fg). Whichever
/// variant reuses more of the current colors wins; ties keep the canonical
/// form. Returns `(glyph, fg, bg)`.
#[inline]
fn pick_glyph(cell: &Cell, cur_fg: Option<Rgb>, cur_bg: Option<Rgb>) -> (char, Rgb, Rgb) {
    match cell.ch {
        HALF_BLOCK => {
            let keep = (cur_fg != Some(cell.fg)) as u8 + (cur_bg != Some(cell.bg)) as u8;
            let flip = (cur_fg != Some(cell.bg)) as u8 + (cur_bg != Some(cell.fg)) as u8;
            if flip < keep { (LOWER_HALF_BLOCK, cell.bg, cell.fg) } else { (HALF_BLOCK, cell.fg, cell.bg) }
        }
        ' ' if cur_bg != Some(cell.bg) && cur_fg == Some(cell.bg) => (FULL_BLOCK, cell.bg, cell.bg),
        _ => (cell.ch, cell.fg, cell.bg),
    }
}

#[inline]
fn push_color(out: &mut Vec<u8>, mode: ColorMode, base: u8, c: Rgb) {
    push_num(out, base as usize);
    match mode {
        ColorMode::TrueColor => {
            out.extend_from_slice(b";2;");
            push_num(out, c.r as usize);
            out.push(b';');
            push_num(out, c.g as usize);
            out.push(b';');
            push_num(out, c.b as usize);
        }
        ColorMode::Ansi256 => {
            out.extend_from_slice(b";5;");
            push_num(out, c.to_ansi256() as usize);
        }
    }
}

/// Appends a decimal number without going through `fmt`.
#[inline]
fn push_num(out: &mut Vec<u8>, mut n: usize) {
    if n < 10 {
        out.push(b'0' + n as u8);
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out.extend_from_slice(&buf[i..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(cols: usize, rows: usize, c: Rgb) -> FrameBuffer {
        let mut fb = FrameBuffer::new(cols, rows * 2);
        fb.clear(c);
        fb
    }

    fn text(r: &Renderer) -> String {
        String::from_utf8(r.output().to_vec()).unwrap()
    }

    #[test]
    fn push_num_formats() {
        let mut v = Vec::new();
        for n in [0, 7, 10, 255, 1234567] {
            v.clear();
            push_num(&mut v, n);
            assert_eq!(String::from_utf8(v.clone()).unwrap(), n.to_string());
        }
    }

    #[test]
    fn uniform_cells_become_spaces() {
        let c = Cell::pixels(Rgb::WHITE, Rgb::WHITE);
        assert_eq!(c.ch, ' ');
        let h = Cell::pixels(Rgb::WHITE, Rgb::BLACK);
        assert_eq!(h, Cell { ch: HALF_BLOCK, fg: Rgb::WHITE, bg: Rgb::BLACK });
    }

    #[test]
    fn first_frame_draws_everything_then_nothing() {
        let mut r = Renderer::new(4, 2);
        let fb = solid(4, 2, Rgb::new(1, 2, 3));
        let s = r.render(&fb, &[]);
        assert_eq!(s.cells_changed, 8);
        assert!(s.bytes > 0);
        let s = r.render(&fb, &[]);
        assert_eq!(s, FrameStats { cells_changed: 0, bytes: 0 });
        assert!(r.output().is_empty());
    }

    #[test]
    fn single_pixel_change_emits_one_cell() {
        let mut r = Renderer::new(10, 5);
        r.set_synchronized(false);
        let mut fb = solid(10, 5, Rgb::BLACK);
        r.render(&fb, &[]);
        fb.set(3, 5, Rgb::new(255, 0, 0)); // bottom half of cell (3, 2)
        let s = r.render(&fb, &[]);
        assert_eq!(s.cells_changed, 1);
        assert_eq!(text(&r), "\x1b[3;4H\x1b[38;2;0;0;0;48;2;255;0;0m\u{2580}");
    }

    #[test]
    fn contiguous_runs_skip_cursor_moves_and_repeat_colors() {
        let mut r = Renderer::new(6, 1);
        r.set_synchronized(false);
        let s = r.render(&solid(6, 1, Rgb::new(9, 9, 9)), &[]);
        assert_eq!(s.cells_changed, 6);
        // One move, one color, six spaces.
        assert_eq!(text(&r), "\x1b[1;1H\x1b[48;2;9;9;9m      ");
    }

    #[test]
    fn gaps_on_a_row_use_cursor_forward() {
        let mut r = Renderer::new(8, 1);
        r.set_synchronized(false);
        let mut fb = solid(8, 1, Rgb::BLACK);
        r.render(&fb, &[]);
        fb.set(1, 0, Rgb::WHITE);
        fb.set(5, 0, Rgb::WHITE);
        r.render(&fb, &[]);
        let out = text(&r);
        assert!(out.starts_with("\x1b[1;2H"), "{out:?}");
        assert!(out.contains("\x1b[3C"), "jump from col 2 to col 5: {out:?}");
        assert_eq!(out.matches("38;2").count(), 1, "colors reused: {out:?}");
    }

    #[test]
    fn swaps_half_blocks_to_reuse_colors() {
        let (a, b) = (Rgb::new(1, 1, 1), Rgb::new(2, 2, 2));
        let cell = Cell::pixels(a, b);
        // Terminal already has fg = b, bg = a: draw the lower half instead.
        assert_eq!(pick_glyph(&cell, Some(b), Some(a)), (LOWER_HALF_BLOCK, b, a));
        assert_eq!(pick_glyph(&cell, Some(a), Some(b)), (HALF_BLOCK, a, b));
        assert_eq!(pick_glyph(&cell, None, None), (HALF_BLOCK, a, b));
        // One-color cell with the right foreground already selected.
        let flat = Cell::pixels(a, a);
        assert_eq!(pick_glyph(&flat, Some(a), Some(b)), (FULL_BLOCK, a, a));
        assert_eq!(pick_glyph(&flat, Some(a), Some(a)).0, ' ');
    }

    #[test]
    fn gradient_rows_need_fewer_color_codes() {
        // Vertical stripes alternating which color is on top: every other
        // cell is the previous one flipped, so no SGR is needed for it.
        let mut r = Renderer::new(8, 1);
        r.set_synchronized(false);
        let mut fb = FrameBuffer::new(8, 2);
        for x in 0..8 {
            let (t, b) = if x % 2 == 0 { (Rgb::WHITE, Rgb::BLACK) } else { (Rgb::BLACK, Rgb::WHITE) };
            fb.set(x, 0, t);
            fb.set(x, 1, b);
        }
        r.render(&fb, &[]);
        assert_eq!(text(&r).matches('m').count(), 1, "one SGR for the whole row");
    }

    #[test]
    fn synchronized_wrapper_only_when_something_changed() {
        let mut r = Renderer::new(2, 1);
        let fb = solid(2, 1, Rgb::WHITE);
        r.render(&fb, &[]);
        let out = text(&r);
        assert!(out.starts_with("\x1b[?2026h") && out.ends_with("\x1b[?2026l"));
        r.render(&fb, &[]);
        assert!(r.output().is_empty());
    }

    #[test]
    fn tolerance_suppresses_small_changes_without_drift() {
        let mut r = Renderer::new(1, 1);
        r.set_tolerance(4);
        r.render(&solid(1, 1, Rgb::new(100, 100, 100)), &[]);
        assert_eq!(r.render(&solid(1, 1, Rgb::new(103, 100, 98)), &[]).cells_changed, 0);
        // Still compared against what is on screen (100), not the last frame.
        assert_eq!(r.render(&solid(1, 1, Rgb::new(104, 100, 100)), &[]).cells_changed, 0);
        assert_eq!(r.render(&solid(1, 1, Rgb::new(105, 100, 100)), &[]).cells_changed, 1);
    }

    #[test]
    fn resize_and_invalidate_force_full_redraw() {
        let mut r = Renderer::new(3, 2);
        let fb = solid(3, 2, Rgb::WHITE);
        r.render(&fb, &[]);
        r.invalidate();
        assert_eq!(r.render(&fb, &[]).cells_changed, 6);
        r.resize(5, 1);
        let fb = solid(5, 1, Rgb::WHITE);
        assert_eq!(r.render(&fb, &[]).cells_changed, 5);
    }

    #[test]
    fn undersized_buffer_reads_black() {
        let mut r = Renderer::new(4, 2);
        let fb = solid(2, 1, Rgb::WHITE);
        r.render(&fb, &[]);
        assert_eq!(r.cell(0, 0).bg, Rgb::WHITE);
        assert_eq!(r.cell(3, 1), Cell::pixels(Rgb::BLACK, Rgb::BLACK));
    }

    #[test]
    fn overlays_clip_and_darken() {
        let mut r = Renderer::new(5, 2);
        let fb = solid(5, 2, Rgb::new(200, 200, 200));
        let o = [Overlay { col: 3, row: 1, text: "abcdef", fg: Rgb::WHITE, bg: None }];
        r.render(&fb, &o);
        assert_eq!(r.cell(3, 1).ch, 'a');
        assert_eq!(r.cell(4, 1).ch, 'b');
        assert!(r.cell(3, 1).bg.luma() < 100, "background is darkened for contrast");
        let off = [Overlay { col: 0, row: 9, text: "x", fg: Rgb::WHITE, bg: None }];
        r.render(&fb, &off); // off-screen row is ignored, not a panic
    }

    #[test]
    fn ansi256_mode_quantizes_and_uses_indexed_sgr() {
        let mut r = Renderer::new(1, 1);
        r.set_synchronized(false);
        r.set_color_mode(ColorMode::Ansi256);
        r.render(&solid(1, 1, Rgb::new(250, 0, 0)), &[]);
        assert_eq!(text(&r), "\x1b[1;1H\x1b[48;5;196m ");
        assert_eq!(r.cell(0, 0).bg, Rgb::new(255, 0, 0));
    }
}
