//! Command-line parsing. Hand-rolled to keep the dependency list to one
//! crate; the surface is small enough that a parser library would be most
//! of the binary.

use crate::render::ColorMode;

pub const USAGE: &str = "\
termplasma - classic demoscene effects in your terminal

USAGE:
    termplasma [OPTIONS]

PLAYBACK:
    -e, --effect <NAME>      start on one effect and stay there (autoplay off)
    -p, --palette <NAME>     force a palette for every effect
        --scene <SECS>       seconds per effect in autoplay [default: 12]
        --fps <N>            frame cap; 0 renders as fast as possible [default: 60]

OUTPUT:
        --colors <MODE>      truecolor, 256 or auto [default: auto]
        --tolerance <N>      skip cells whose color moved by at most N per
                             channel; trades accuracy for bandwidth [default: 0]
        --no-hud             start with the status line hidden
        --no-sync            do not wrap frames in synchronized-update markers

HEADLESS:
        --headless           render to memory instead of the terminal
        --frames <N>         frames to render headless [default: 600]
        --size <COLSxROWS>   headless terminal size in cells [default: 120x40]
        --ascii              after a headless run, print the last frame as ASCII
        --bench              time every effect headless and print a table

    -l, --list               list effects and palettes
    -h, --help               print this help
    -V, --version            print the version

Keys while running: h shows the full list.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Fixed(ColorMode),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub effect: Option<String>,
    pub palette: Option<String>,
    pub scene_secs: f32,
    pub fps: u32,
    pub colors: ColorChoice,
    pub tolerance: u8,
    pub hud: bool,
    pub sync: bool,
    pub headless: bool,
    pub frames: u64,
    pub size: (usize, usize),
    pub ascii: bool,
    pub bench: bool,
    pub list: bool,
    pub help: bool,
    pub version: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            effect: None,
            palette: None,
            scene_secs: crate::demo::DEFAULT_SCENE_SECS,
            fps: 60,
            colors: ColorChoice::Auto,
            tolerance: 0,
            hud: true,
            sync: true,
            headless: false,
            frames: 600,
            size: (120, 40),
            ascii: false,
            bench: false,
            list: false,
            help: false,
            version: false,
        }
    }
}

/// Parses arguments (without the program name).
pub fn parse<I, S>(args: I) -> Result<Options, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut o = Options::default();
    let mut it = args.into_iter().map(Into::into);
    while let Some(arg) = it.next() {
        // Accept both `--flag value` and `--flag=value`.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "-e" | "--effect" => o.effect = Some(value(&flag)?),
            "-p" | "--palette" => o.palette = Some(value(&flag)?),
            "--scene" => o.scene_secs = number(&flag, &value(&flag)?)?,
            "--fps" => o.fps = number(&flag, &value(&flag)?)?,
            "--tolerance" => o.tolerance = number(&flag, &value(&flag)?)?,
            "--frames" => o.frames = number(&flag, &value(&flag)?)?,
            "--size" => o.size = size(&value(&flag)?)?,
            "--colors" => {
                o.colors = match value(&flag)?.to_ascii_lowercase().as_str() {
                    "auto" => ColorChoice::Auto,
                    "truecolor" | "24bit" | "24-bit" => ColorChoice::Fixed(ColorMode::TrueColor),
                    "256" => ColorChoice::Fixed(ColorMode::Ansi256),
                    other => return Err(format!("unknown color mode '{other}' (truecolor, 256, auto)")),
                }
            }
            "--no-hud" => o.hud = false,
            "--no-sync" => o.sync = false,
            "--headless" => o.headless = true,
            "--ascii" => o.ascii = true,
            "--bench" => o.bench = true,
            "-l" | "--list" => o.list = true,
            "-h" | "--help" => o.help = true,
            "-V" | "--version" => o.version = true,
            other => return Err(format!("unknown argument '{other}' (try --help)")),
        }
    }
    if o.ascii && !o.headless {
        return Err("--ascii only applies to --headless runs".into());
    }
    if !(o.scene_secs.is_finite() && o.scene_secs >= 1.0) {
        return Err("--scene must be at least 1 second".into());
    }
    Ok(o)
}

fn number<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse().map_err(|_| format!("{flag}: '{v}' is not a valid number"))
}

fn size(v: &str) -> Result<(usize, usize), String> {
    let parsed = v
        .split_once(['x', 'X'])
        .and_then(|(c, r)| Some((c.parse::<usize>().ok()?, r.parse::<usize>().ok()?)));
    match parsed {
        Some((c, r)) if (1..=2000).contains(&c) && (1..=1000).contains(&r) => Ok((c, r)),
        _ => Err(format!("--size: '{v}' should look like 120x40")),
    }
}

/// Picks a color mode from the environment: truecolor unless the terminal
/// is known not to support it.
pub fn detect_color_mode(
    colorterm: Option<&str>,
    term_program: Option<&str>,
    term: Option<&str>,
) -> ColorMode {
    let colorterm = colorterm.unwrap_or("").to_ascii_lowercase();
    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        return ColorMode::TrueColor;
    }
    // Apple Terminal before macOS 26 and the Linux console mangle 24-bit SGR.
    if term_program == Some("Apple_Terminal") || term == Some("linux") {
        return ColorMode::Ansi256;
    }
    ColorMode::TrueColor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let o = parse(Vec::<String>::new()).unwrap();
        assert_eq!(o, Options::default());
        assert_eq!(o.fps, 60);
        assert!(o.hud && o.sync && !o.headless);
    }

    #[test]
    fn values_both_styles() {
        let o = parse(["--effect", "fire", "--fps=30", "-p", "amber", "--size", "80x24"]).unwrap();
        assert_eq!(o.effect.as_deref(), Some("fire"));
        assert_eq!(o.palette.as_deref(), Some("amber"));
        assert_eq!(o.fps, 30);
        assert_eq!(o.size, (80, 24));
    }

    #[test]
    fn headless_flags() {
        let o = parse(["--headless", "--frames", "120", "--ascii", "--tolerance", "3"]).unwrap();
        assert!(o.headless && o.ascii);
        assert_eq!(o.frames, 120);
        assert_eq!(o.tolerance, 3);
    }

    #[test]
    fn color_modes() {
        assert_eq!(parse(["--colors", "256"]).unwrap().colors, ColorChoice::Fixed(ColorMode::Ansi256));
        assert_eq!(parse(["--colors=TrueColor"]).unwrap().colors, ColorChoice::Fixed(ColorMode::TrueColor));
        assert!(parse(["--colors", "16"]).is_err());
    }

    #[test]
    fn errors_are_specific() {
        assert!(parse(["--fps"]).unwrap_err().contains("needs a value"));
        assert!(parse(["--fps", "fast"]).unwrap_err().contains("not a valid number"));
        assert!(parse(["--tolerance", "300"]).is_err());
        assert!(parse(["--size", "80"]).is_err());
        assert!(parse(["--size", "0x10"]).is_err());
        assert!(parse(["--scene", "0.5"]).is_err());
        assert!(parse(["--ascii"]).unwrap_err().contains("--headless"));
        assert!(parse(["--wat"]).unwrap_err().contains("--wat"));
    }

    #[test]
    fn color_detection() {
        use ColorMode::*;
        assert_eq!(detect_color_mode(Some("truecolor"), Some("Apple_Terminal"), None), TrueColor);
        assert_eq!(detect_color_mode(None, Some("Apple_Terminal"), None), Ansi256);
        assert_eq!(detect_color_mode(None, None, Some("linux")), Ansi256);
        assert_eq!(detect_color_mode(None, Some("iTerm.app"), Some("xterm-256color")), TrueColor);
    }
}
