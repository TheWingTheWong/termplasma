//! The demo engine: sequences effects like an intro, crossfades between
//! them, and owns playback state (pause, speed, palette choice).
//!
//! It knows nothing about terminals. The interactive app and the headless
//! runner both drive it the same way: `update(dt)`, then `render(fb)`.

use crate::effects::{Effect, Frame};
use crate::framebuffer::FrameBuffer;
use crate::palette::{self, Palette};

/// Seconds each effect holds the screen in autoplay.
pub const DEFAULT_SCENE_SECS: f32 = 12.0;
/// Crossfade length for autoplay scene changes.
const AUTO_FADE_SECS: f32 = 1.6;
/// Crossfade length when the user switches effects.
const MANUAL_FADE_SECS: f32 = 0.45;

const SPEEDS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0];

#[derive(Clone, Copy, Debug)]
struct Transition {
    to: usize,
    progress: f32,
    length: f32,
}

pub struct Demo {
    effects: Vec<Box<dyn Effect>>,
    palettes: Vec<Palette>,
    /// Each effect's preferred palette, resolved once.
    preferred: Vec<usize>,
    current: usize,
    transition: Option<Transition>,
    palette_override: Option<usize>,
    autoplay: bool,
    paused: bool,
    speed_index: usize,
    scene_secs: f32,
    time: f32,
    /// Per-effect clocks: an effect's time starts at zero each time it is
    /// brought on screen, so the scroller always opens on its first word.
    clocks: Vec<f32>,
    scene_time: f32,
    dt: f32,
    scratch: FrameBuffer,
}

impl Demo {
    pub fn new(effects: Vec<Box<dyn Effect>>, palettes: Vec<Palette>) -> Demo {
        assert!(!effects.is_empty(), "a demo needs at least one effect");
        assert!(!palettes.is_empty(), "a demo needs at least one palette");
        let clocks = vec![0.0; effects.len()];
        let preferred = effects.iter().map(|e| palette::find(&palettes, e.palette()).unwrap_or(0)).collect();
        Demo {
            effects,
            palettes,
            preferred,
            current: 0,
            transition: None,
            palette_override: None,
            autoplay: true,
            paused: false,
            speed_index: 3,
            scene_secs: DEFAULT_SCENE_SECS,
            time: 0.0,
            clocks,
            scene_time: 0.0,
            dt: 0.0,
            scratch: FrameBuffer::default(),
        }
    }

    /// The standard demo: every built-in effect and palette.
    pub fn standard() -> Demo {
        Demo::new(crate::effects::all(), palette::builtin())
    }

    pub fn set_scene_secs(&mut self, secs: f32) {
        self.scene_secs = secs.max(1.0);
    }

    pub fn set_autoplay(&mut self, on: bool) {
        self.autoplay = on;
        self.scene_time = 0.0;
    }

    /// Advances the clock by `real_dt` seconds of wall time.
    pub fn update(&mut self, real_dt: f32) {
        let dt = if self.paused { 0.0 } else { real_dt.clamp(0.0, 0.25) * self.speed() };
        self.dt = dt;
        self.time += dt;
        self.clocks[self.current] += dt;
        if let Some(tr) = self.transition {
            self.clocks[tr.to] += dt;
        }

        if let Some(mut tr) = self.transition {
            tr.progress += dt / tr.length;
            if tr.progress >= 1.0 {
                self.current = tr.to;
                self.transition = None;
                self.scene_time = 0.0;
            } else {
                self.transition = Some(tr);
            }
        } else {
            self.scene_time += dt;
            if self.autoplay && self.scene_time >= self.scene_secs && self.effects.len() > 1 {
                self.start_transition((self.current + 1) % self.effects.len(), AUTO_FADE_SECS);
            }
        }
    }

    /// Draws the current frame (two effects blended during a transition).
    pub fn render(&mut self, fb: &mut FrameBuffer) {
        let frame = Frame { time: self.clocks[self.current], dt: self.dt };
        let pal = self.palette_for(self.current);
        self.effects[self.current].render(fb, frame, &self.palettes[pal]);

        if let Some(tr) = self.transition {
            self.scratch.resize(fb.width(), fb.height());
            let pal = self.palette_for(tr.to);
            let frame = Frame { time: self.clocks[tr.to], dt: self.dt };
            self.effects[tr.to].render(&mut self.scratch, frame, &self.palettes[pal]);
            fb.blend_toward(&self.scratch, (smoothstep(tr.progress) * 256.0) as u16);
        }
    }

    fn start_transition(&mut self, to: usize, length: f32) {
        // A new request mid-fade lands the old one first.
        if let Some(tr) = self.transition.take() {
            self.current = tr.to;
        }
        self.scene_time = 0.0;
        if to != self.current {
            self.clocks[to] = 0.0;
            self.transition = Some(Transition { to, progress: 0.0, length });
        }
    }

    /// The effect being faded to, or the current one.
    fn target(&self) -> usize {
        self.transition.map_or(self.current, |t| t.to)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.effects.len() {
            self.start_transition(index, MANUAL_FADE_SECS);
        }
    }

    pub fn select_name(&mut self, name: &str) -> bool {
        match self.effects.iter().position(|e| e.name().eq_ignore_ascii_case(name)) {
            Some(i) => {
                self.current = i;
                self.clocks[i] = 0.0;
                self.transition = None;
                self.scene_time = 0.0;
                true
            }
            None => false,
        }
    }

    pub fn next_effect(&mut self) {
        self.select((self.target() + 1) % self.effects.len());
    }

    pub fn prev_effect(&mut self) {
        let n = self.effects.len();
        self.select((self.target() + n - 1) % n);
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn toggle_autoplay(&mut self) {
        self.set_autoplay(!self.autoplay);
    }

    pub fn faster(&mut self) {
        self.speed_index = (self.speed_index + 1).min(SPEEDS.len() - 1);
    }

    pub fn slower(&mut self) {
        self.speed_index = self.speed_index.saturating_sub(1);
    }

    /// Cycles the palette override forward, starting from whatever the
    /// on-screen effect is using now.
    pub fn next_palette(&mut self) {
        let cur = self.palette_for(self.target());
        self.palette_override = Some((cur + 1) % self.palettes.len());
    }

    pub fn prev_palette(&mut self) {
        let n = self.palettes.len();
        let cur = self.palette_for(self.target());
        self.palette_override = Some((cur + n - 1) % n);
    }

    /// Goes back to each effect's own palette.
    pub fn auto_palette(&mut self) {
        self.palette_override = None;
    }

    pub fn set_palette(&mut self, name: &str) -> bool {
        match palette::find(&self.palettes, name) {
            Some(i) => {
                self.palette_override = Some(i);
                true
            }
            None => false,
        }
    }

    fn palette_for(&self, effect: usize) -> usize {
        self.palette_override.unwrap_or(self.preferred[effect])
    }

    // --- read-only state for the HUD -------------------------------------

    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn effect_index(&self) -> usize {
        self.target()
    }

    pub fn effect_name(&self) -> &'static str {
        self.effects[self.target()].name()
    }

    pub fn effect_names(&self) -> Vec<&'static str> {
        self.effects.iter().map(|e| e.name()).collect()
    }

    pub fn palette_name(&self) -> &'static str {
        self.palettes[self.palette_for(self.target())].name()
    }

    pub fn palette_is_override(&self) -> bool {
        self.palette_override.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_autoplay(&self) -> bool {
        self.autoplay
    }

    pub fn in_transition(&self) -> bool {
        self.transition.is_some()
    }

    pub fn speed(&self) -> f32 {
        SPEEDS[self.speed_index]
    }

    /// Total demo time (scaled by speed, frozen while paused).
    pub fn time(&self) -> f32 {
        self.time
    }
}

/// Hermite ease, so fades start and land softly.
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    /// Fills the screen with a fixed color and counts its renders.
    struct Solid(&'static str, Rgb, std::rc::Rc<std::cell::Cell<u32>>);

    impl Effect for Solid {
        fn name(&self) -> &'static str {
            self.0
        }
        fn palette(&self) -> &'static str {
            "doom"
        }
        fn render(&mut self, fb: &mut FrameBuffer, _: Frame, _: &Palette) {
            self.2.set(self.2.get() + 1);
            fb.clear(self.1);
        }
    }

    /// Advances in 60 Hz steps, like the real frame loop.
    fn run(d: &mut Demo, secs: f32) {
        for _ in 0..(secs * 60.0).round() as usize {
            d.update(1.0 / 60.0);
        }
    }

    fn demo() -> (Demo, std::rc::Rc<std::cell::Cell<u32>>) {
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let effects: Vec<Box<dyn Effect>> = vec![
            Box::new(Solid("black", Rgb::BLACK, count.clone())),
            Box::new(Solid("white", Rgb::WHITE, count.clone())),
            Box::new(Solid("red", Rgb::new(255, 0, 0), count.clone())),
        ];
        (Demo::new(effects, palette::builtin()), count)
    }

    #[test]
    fn autoplay_advances_with_a_crossfade() {
        let (mut d, _) = demo();
        d.set_scene_secs(2.0);
        let mut fb = FrameBuffer::new(4, 4);
        run(&mut d, 1.9);
        assert_eq!(d.effect_index(), 0);
        assert!(!d.in_transition());
        run(&mut d, 0.2);
        assert!(d.in_transition());
        assert_eq!(d.effect_index(), 1, "HUD shows the incoming effect");

        // Halfway through the fade the frame is a blend.
        run(&mut d, AUTO_FADE_SECS / 2.0 - 0.1);
        d.render(&mut fb);
        let mid = fb.get(0, 0).unwrap();
        assert!(mid.r > 60 && mid.r < 200, "mid-fade gray, got {mid:?}");

        run(&mut d, AUTO_FADE_SECS);
        assert!(!d.in_transition());
        d.render(&mut fb);
        assert_eq!(fb.get(0, 0), Some(Rgb::WHITE));
    }

    #[test]
    fn manual_selection_and_wrapping() {
        let (mut d, _) = demo();
        d.prev_effect();
        assert_eq!(d.effect_name(), "red");
        d.next_effect(); // mid-fade: lands red, heads to black
        assert_eq!(d.effect_name(), "black");
        run(&mut d, 1.0);
        assert!(!d.in_transition());
        d.select(7);
        assert_eq!(d.effect_name(), "black", "out-of-range index ignored");
        assert!(d.select_name("WHITE"));
        assert_eq!(d.effect_name(), "white");
        assert!(!d.select_name("nope"));
    }

    #[test]
    fn only_rendering_effects_run_and_pause_freezes_time() {
        let (mut d, count) = demo();
        let mut fb = FrameBuffer::new(2, 2);
        d.render(&mut fb);
        assert_eq!(count.get(), 1, "one effect outside transitions");
        d.next_effect();
        d.render(&mut fb);
        assert_eq!(count.get(), 3, "two effects during a crossfade");

        d.toggle_pause();
        let t = d.time();
        d.update(1.0);
        assert_eq!(d.time(), t);
        assert!(d.in_transition(), "a paused fade stays paused");
    }

    #[test]
    fn palette_override_cycles_and_resets() {
        let (mut d, _) = demo();
        assert_eq!(d.palette_name(), "doom");
        assert!(!d.palette_is_override());
        d.next_palette();
        assert_eq!(d.palette_name(), "copper");
        d.prev_palette();
        d.prev_palette();
        assert_eq!(d.palette_name(), "velvet");
        d.auto_palette();
        assert_eq!(d.palette_name(), "doom");
        assert!(d.set_palette("amber"));
        assert!(!d.set_palette("nope"));
    }

    #[test]
    fn speed_is_clamped() {
        let (mut d, _) = demo();
        for _ in 0..20 {
            d.faster();
        }
        assert_eq!(d.speed(), 3.0);
        for _ in 0..20 {
            d.slower();
        }
        assert_eq!(d.speed(), 0.25);
        d.update(0.2);
        assert!((d.time() - 0.05).abs() < 1e-6);
    }

    #[test]
    fn effects_start_their_clock_when_shown() {
        struct Clock(std::rc::Rc<std::cell::Cell<f32>>);
        impl Effect for Clock {
            fn name(&self) -> &'static str {
                "clock"
            }
            fn palette(&self) -> &'static str {
                "doom"
            }
            fn render(&mut self, _: &mut FrameBuffer, f: Frame, _: &Palette) {
                self.0.set(f.time);
            }
        }
        let seen = std::rc::Rc::new(std::cell::Cell::new(-1.0));
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let effects: Vec<Box<dyn Effect>> =
            vec![Box::new(Solid("a", Rgb::BLACK, count)), Box::new(Clock(seen.clone()))];
        let mut d = Demo::new(effects, palette::builtin());
        let mut fb = FrameBuffer::new(2, 2);
        run(&mut d, 5.0);
        d.next_effect();
        d.render(&mut fb);
        assert_eq!(seen.get(), 0.0, "fresh clock on entry");
        run(&mut d, 1.0);
        d.render(&mut fb);
        assert!((seen.get() - 1.0).abs() < 0.02, "clock runs while shown: {}", seen.get());
    }

    #[test]
    fn huge_frame_gaps_are_clamped() {
        let (mut d, _) = demo();
        d.update(30.0);
        assert!(d.time() <= 0.25 + f32::EPSILON);
    }

    #[test]
    fn standard_demo_has_all_effects() {
        let d = Demo::standard();
        assert_eq!(d.effect_count(), 7);
    }
}
