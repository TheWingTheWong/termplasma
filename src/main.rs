use std::io::{self, IsTerminal};
use std::process::ExitCode;

use termplasma::app::{self, Settings};
use termplasma::cli::{self, ColorChoice, Options};
use termplasma::demo::Demo;
use termplasma::render::{ColorMode, Renderer};
use termplasma::{effects, headless, hud, palette, preview};

fn main() -> ExitCode {
    let opts = match cli::parse(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("termplasma: {e}");
            return ExitCode::from(2);
        }
    };
    match run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("termplasma: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(opts: Options) -> Result<(), String> {
    if opts.help {
        print!("{}", cli::USAGE);
        return Ok(());
    }
    if opts.version {
        println!("termplasma {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if opts.list {
        list();
        return Ok(());
    }
    if opts.bench {
        bench(&opts);
        return Ok(());
    }

    let mut demo = Demo::standard();
    demo.set_scene_secs(opts.scene_secs);
    if let Some(name) = &opts.effect {
        if !demo.select_name(name) {
            return Err(format!("unknown effect '{name}' (see --list)"));
        }
        demo.set_autoplay(false);
    }
    if let Some(name) = &opts.palette {
        if !demo.set_palette(name) {
            return Err(format!("unknown palette '{name}' (see --list)"));
        }
    }
    let color_mode = match opts.colors {
        ColorChoice::Fixed(m) => m,
        ColorChoice::Auto => detect_color_mode(),
    };

    if opts.headless {
        return headless_run(&opts, &mut demo, color_mode);
    }
    if !io::stdout().is_terminal() {
        return Err("stdout is not a terminal; use --headless to render without one".into());
    }
    let settings =
        Settings { fps: opts.fps, hud: opts.hud, color_mode, tolerance: opts.tolerance, sync: opts.sync };
    app::run(&mut demo, settings).map_err(|e| format!("terminal error: {e}"))
}

fn detect_color_mode() -> ColorMode {
    let var = |k: &str| std::env::var(k).ok();
    cli::detect_color_mode(
        var("COLORTERM").as_deref(),
        var("TERM_PROGRAM").as_deref(),
        var("TERM").as_deref(),
    )
}

/// The fixed step used for headless runs: the frame cap, or 60 Hz.
fn headless_dt(opts: &Options) -> f32 {
    1.0 / if opts.fps > 0 { opts.fps as f32 } else { 60.0 }
}

fn headless_run(opts: &Options, demo: &mut Demo, color_mode: ColorMode) -> Result<(), String> {
    let (cols, rows) = opts.size;
    let mut renderer = Renderer::new(cols, rows);
    renderer.set_color_mode(color_mode);
    renderer.set_tolerance(opts.tolerance);
    renderer.set_synchronized(opts.sync);
    let (report, fb) = headless::run(demo, &mut renderer, opts.frames, headless_dt(opts));

    if opts.ascii {
        print!("{}", preview::to_ascii(&fb));
    }
    if let Some(path) = &opts.ppm {
        std::fs::write(path, preview::to_ppm(&fb, 4)).map_err(|e| format!("writing {path}: {e}"))?;
    }
    let r = &report;
    println!(
        "{} frames at {}x{} cells ({}x{} px), {:?}, ended on {} / {}",
        r.frames,
        r.cols,
        r.rows,
        r.cols,
        r.rows * 2,
        color_mode,
        demo.effect_name(),
        demo.palette_name()
    );
    println!(
        "effects {:.3} ms/frame, encode {:.3} ms/frame, {:.0} fps without terminal I/O",
        r.effect_ms(),
        r.encode_ms(),
        r.fps()
    );
    println!(
        "{}/frame on the wire, {:.1}% of cells re-sent",
        hud::format_bytes(r.bytes_per_frame() as f32),
        r.change_ratio() * 100.0
    );
    Ok(())
}

fn bench(opts: &Options) {
    let (cols, rows) = opts.size;
    let frames = opts.frames;
    // Benchmarks should not depend on the environment: auto means truecolor.
    let mode = match opts.colors {
        ColorChoice::Fixed(m) => m,
        ColorChoice::Auto => ColorMode::TrueColor,
    };
    println!("{frames} frames per effect at {cols}x{rows} cells ({cols}x{} px), {mode:?}", rows * 2);
    println!();
    println!(
        "{:<10} {:>10} {:>10} {:>9} {:>11} {:>9}",
        "effect", "effect ms", "encode ms", "fps", "bytes/frame", "re-sent"
    );
    let names: Vec<&str> = effects::all().iter().map(|e| e.name()).collect();
    for name in names {
        let mut demo = Demo::standard();
        demo.select_name(name);
        demo.set_autoplay(false);
        let mut renderer = Renderer::new(cols, rows);
        renderer.set_tolerance(opts.tolerance);
        renderer.set_color_mode(mode);
        let (r, _) = headless::run(&mut demo, &mut renderer, frames, headless_dt(opts));
        println!(
            "{:<10} {:>10.3} {:>10.3} {:>9.0} {:>11} {:>8.1}%",
            name,
            r.effect_ms(),
            r.encode_ms(),
            r.fps(),
            hud::format_bytes(r.bytes_per_frame() as f32),
            r.change_ratio() * 100.0
        );
    }
}

fn list() {
    println!("effects (demo order, keys 1-{}):", effects::all().len());
    for (i, e) in effects::all().iter().enumerate() {
        println!("  {}  {:<10} palette: {}", i + 1, e.name(), e.palette());
    }
    println!();
    println!("palettes (p / P to cycle):");
    for p in palette::builtin() {
        println!("  {}", p.name());
    }
}
