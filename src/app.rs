//! The interactive terminal front end.
//!
//! Terminal state is owned by [`TerminalGuard`]: raw mode, the alternate
//! screen and the hidden cursor are switched on when it is created and
//! restored when it drops, which covers normal exit, `?` error returns and
//! unwinding panics. A panic hook restores the terminal before the panic
//! message prints, so the message is readable. In raw mode Ctrl-C arrives as
//! a key event instead of SIGINT, and is handled like `q`.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};

use crate::demo::Demo;
use crate::framebuffer::FrameBuffer;
use crate::hud::{self, Meter};
use crate::render::{ColorMode, Overlay, Renderer};

/// Everything the keyboard can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Next,
    Prev,
    Select(usize),
    Pause,
    Autoplay,
    NextPalette,
    PrevPalette,
    AutoPalette,
    Faster,
    Slower,
    ToggleHud,
    ToggleHelp,
}

/// Maps a key press to an action. Releases and repeats of non-navigation
/// keys are ignored, so terminals that report key-up events behave the same.
pub fn key_action(key: KeyEvent) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')).then_some(Action::Quit);
    }
    Some(match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('n') | KeyCode::Right | KeyCode::Down => Action::Next,
        KeyCode::Char('b') | KeyCode::Left | KeyCode::Up => Action::Prev,
        KeyCode::Char(c @ '1'..='9') => Action::Select(c as usize - '1' as usize),
        KeyCode::Char(' ') => Action::Pause,
        KeyCode::Char('a') => Action::Autoplay,
        KeyCode::Char('p') => Action::NextPalette,
        KeyCode::Char('P') => Action::PrevPalette,
        KeyCode::Char('0') => Action::AutoPalette,
        KeyCode::Char(']') | KeyCode::Char('+') | KeyCode::Char('=') => Action::Faster,
        KeyCode::Char('[') | KeyCode::Char('-') => Action::Slower,
        KeyCode::Char('f') => Action::ToggleHud,
        KeyCode::Char('h') | KeyCode::Char('?') => Action::ToggleHelp,
        _ => return None,
    })
}

pub struct Settings {
    pub fps: u32,
    pub hud: bool,
    pub color_mode: ColorMode,
    pub tolerance: u8,
    pub sync: bool,
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(out: &mut impl Write) -> io::Result<TerminalGuard> {
        terminal::enable_raw_mode()?;
        // From here on, any failure must still restore the terminal.
        let guard = TerminalGuard;
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::DisableLineWrap)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let mut out = io::stdout();
    // Reset colors first so the shell does not inherit the last background.
    let _ = out.write_all(b"\x1b[0m");
    let _ = execute!(out, terminal::EnableLineWrap, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default(info);
    }));
}

/// Runs the demo in the terminal until the user quits.
pub fn run(demo: &mut Demo, settings: Settings) -> io::Result<()> {
    let mut out = io::stdout().lock();
    install_panic_hook();
    let _guard = TerminalGuard::enter(&mut out)?;

    let (cols, rows) = terminal::size()?;
    let (mut cols, mut rows) = (cols as usize, rows as usize);
    let mut fb = FrameBuffer::new(cols, rows * 2);
    let mut renderer = Renderer::new(cols, rows);
    renderer.set_color_mode(settings.color_mode);
    renderer.set_tolerance(settings.tolerance);
    renderer.set_synchronized(settings.sync);

    let frame_time = (settings.fps > 0).then(|| Duration::from_secs_f64(1.0 / settings.fps as f64));
    let mut show_hud = settings.hud;
    let mut show_help = false;
    let help_lines = hud::help_text();
    let mut meter = Meter::default();
    let mut last = Instant::now();
    let mut deadline = last;

    loop {
        // Handle input until it is time for the next frame.
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if !event::poll(wait)? {
                break;
            }
            match event::read()? {
                Event::Key(key) => match key_action(key) {
                    Some(Action::Quit) => return Ok(()),
                    Some(Action::ToggleHud) => show_hud = !show_hud,
                    Some(Action::ToggleHelp) => show_help = !show_help,
                    Some(action) => apply(demo, action),
                    None => {}
                },
                Event::Resize(c, r) => {
                    (cols, rows) = (c as usize, r as usize);
                    fb.resize(cols, rows * 2);
                    renderer.resize(cols, rows);
                }
                _ => {}
            }
        }

        let now = Instant::now();
        demo.update(now.duration_since(last).as_secs_f32());
        last = now;
        demo.render(&mut fb);

        let status = if show_hud { hud::status_text(demo, &meter) } else { String::new() };
        let mut overlays: Vec<Overlay> = Vec::new();
        if show_hud {
            overlays.push(hud::status_overlay(&status, rows));
        }
        if show_help {
            overlays.extend(hud::help_overlays(&help_lines, cols, rows));
        }
        let stats = renderer.render(&fb, &overlays);
        out.write_all(renderer.output())?;
        out.flush()?;
        meter.tick(Instant::now(), stats);

        deadline = match frame_time {
            // Keep a steady cadence, but do not try to catch up after a stall.
            Some(ft) => (deadline + ft).max(Instant::now()),
            None => Instant::now(),
        };
    }
}

fn apply(demo: &mut Demo, action: Action) {
    match action {
        Action::Next => demo.next_effect(),
        Action::Prev => demo.prev_effect(),
        Action::Select(i) => demo.select(i),
        Action::Pause => demo.toggle_pause(),
        Action::Autoplay => demo.toggle_autoplay(),
        Action::NextPalette => demo.next_palette(),
        Action::PrevPalette => demo.prev_palette(),
        Action::AutoPalette => demo.auto_palette(),
        Action::Faster => demo.faster(),
        Action::Slower => demo.slower(),
        Action::Quit | Action::ToggleHud | Action::ToggleHelp => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn quitting() {
        assert_eq!(key_action(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(key_action(key(KeyCode::Esc)), Some(Action::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_action(ctrl_c), Some(Action::Quit));
        let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(key_action(ctrl_x), None);
    }

    #[test]
    fn navigation_and_digits() {
        assert_eq!(key_action(key(KeyCode::Right)), Some(Action::Next));
        assert_eq!(key_action(key(KeyCode::Char('b'))), Some(Action::Prev));
        assert_eq!(key_action(key(KeyCode::Char('1'))), Some(Action::Select(0)));
        assert_eq!(key_action(key(KeyCode::Char('7'))), Some(Action::Select(6)));
        assert_eq!(key_action(key(KeyCode::Char('0'))), Some(Action::AutoPalette));
        assert_eq!(key_action(key(KeyCode::Char('P'))), Some(Action::PrevPalette));
        assert_eq!(key_action(key(KeyCode::Char('z'))), None);
    }

    #[test]
    fn releases_are_ignored() {
        let mut k = key(KeyCode::Char('n'));
        k.kind = KeyEventKind::Release;
        assert_eq!(key_action(k), None);
    }

    #[test]
    fn actions_reach_the_demo() {
        let mut d = Demo::standard();
        apply(&mut d, Action::Select(4));
        assert_eq!(d.effect_index(), 4);
        apply(&mut d, Action::Pause);
        assert!(d.is_paused());
        apply(&mut d, Action::Autoplay);
        assert!(!d.is_autoplay());
    }
}
