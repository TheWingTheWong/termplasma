//! Every effect must render at any size without panicking, survive resizes
//! mid-run, and actually draw something.

use std::collections::HashSet;

use termplasma::color::Rgb;
use termplasma::demo::Demo;
use termplasma::effects::{self, Frame};
use termplasma::framebuffer::FrameBuffer;
use termplasma::palette;
use termplasma::render::Renderer;

const SIZES: &[(usize, usize)] = &[
    (0, 0),
    (0, 10),
    (10, 0),
    (1, 1),
    (1, 2),
    (2, 1),
    (3, 7),
    (7, 3),
    (17, 9),
    (80, 48),
    (240, 120),
    (400, 20),
    (20, 300),
];

/// Frame timings that stress the effects: normal, paused, huge, tiny.
const STEPS: &[f32] = &[1.0 / 60.0, 0.0, 0.5, 1e-4, 1.0 / 30.0];

#[test]
fn every_effect_renders_at_every_size() {
    let pals = palette::builtin();
    for mut effect in effects::all() {
        for &(w, h) in SIZES {
            let mut fb = FrameBuffer::new(w, h);
            let mut time = 0.0;
            for &dt in STEPS {
                time += dt;
                effect.render(&mut fb, Frame { time, dt }, &pals[0]);
            }
            assert_eq!(fb.pixels().len(), w * h, "{} changed the buffer size", effect.name());
        }
    }
}

#[test]
fn every_effect_survives_resizing_one_instance() {
    let pals = palette::builtin();
    for mut effect in effects::all() {
        let mut fb = FrameBuffer::default();
        // Grow, shrink, degenerate, grow again: tables must be rebuilt.
        for (i, &(w, h)) in SIZES.iter().chain(SIZES.iter().rev()).enumerate() {
            fb.resize(w, h);
            let frame = Frame { time: i as f32 * 0.37, dt: 1.0 / 60.0 };
            effect.render(&mut fb, frame, &pals[i % pals.len()]);
        }
    }
}

#[test]
fn every_effect_survives_long_running_time() {
    // Large times stress fixed-point wrapping and float precision paths.
    let pals = palette::builtin();
    for mut effect in effects::all() {
        let mut fb = FrameBuffer::new(64, 40);
        for t in [1e3, 1e5, 3.6e6, -5.0] {
            effect.render(&mut fb, Frame { time: t, dt: 1.0 / 60.0 }, &pals[1]);
        }
    }
}

#[test]
fn every_effect_draws_a_real_picture() {
    let pals = palette::builtin();
    for mut effect in effects::all() {
        let p = palette::find(&pals, effect.palette()).expect("preferred palette exists");
        let mut fb = FrameBuffer::new(120, 80);
        fb.clear(Rgb::new(1, 2, 3));
        for i in 0..90 {
            let frame = Frame { time: 2.0 + i as f32 / 60.0, dt: 1.0 / 60.0 };
            effect.render(&mut fb, frame, &pals[p]);
        }
        let distinct: HashSet<Rgb> = fb.pixels().iter().copied().collect();
        assert!(distinct.len() >= 4, "{} drew only {} distinct colors", effect.name(), distinct.len());
        assert!(
            !fb.pixels().contains(&Rgb::new(1, 2, 3)),
            "{} left stale pixels from the previous frame",
            effect.name()
        );
    }
}

#[test]
fn every_effect_animates() {
    let pals = palette::builtin();
    for mut effect in effects::all() {
        let mut a = FrameBuffer::new(60, 40);
        let mut b = FrameBuffer::new(60, 40);
        effect.render(&mut a, Frame { time: 1.0, dt: 1.0 / 60.0 }, &pals[0]);
        for i in 1..=30 {
            effect.render(&mut b, Frame { time: 1.0 + i as f32 / 60.0, dt: 1.0 / 60.0 }, &pals[0]);
        }
        assert_ne!(a.pixels(), b.pixels(), "{} is static", effect.name());
    }
}

#[test]
fn effects_follow_the_palette() {
    // Rendering the same frame with two palettes must give different
    // pictures: no effect may hard-code its colors.
    let pals = palette::builtin();
    for name in effects::all().iter().map(|e| e.name()) {
        let render = |p: &palette::Palette| {
            let mut e = effects::all().into_iter().find(|e| e.name() == name).unwrap();
            let mut fb = FrameBuffer::new(50, 30);
            e.render(&mut fb, Frame { time: 3.0, dt: 1.0 / 60.0 }, p);
            fb
        };
        assert_ne!(render(&pals[0]).pixels(), render(&pals[3]).pixels(), "{name} ignores the palette");
    }
}

#[test]
fn effect_names_are_unique_and_palettes_exist() {
    let pals = palette::builtin();
    let all = effects::all();
    let names: HashSet<_> = all.iter().map(|e| e.name()).collect();
    assert_eq!(names.len(), all.len());
    assert_eq!(all.len(), 7);
    for e in &all {
        assert!(palette::find(&pals, e.palette()).is_some(), "{} wants a missing palette", e.name());
    }
}

#[test]
fn full_demo_through_the_renderer_at_odd_sizes() {
    for &(cols, rows) in &[(1, 1), (5, 2), (33, 11), (160, 50)] {
        let mut demo = Demo::standard();
        demo.set_scene_secs(1.0);
        let mut fb = FrameBuffer::new(cols, rows * 2);
        let mut r = Renderer::new(cols, rows);
        // Long enough to pass through every scene and crossfade.
        for _ in 0..(7 * 3 * 60) {
            demo.update(1.0 / 60.0);
            demo.render(&mut fb);
            r.render(&fb, &[]);
        }
    }
}
