//! CLI-Argument-Parsing mit `clap`.

use clap::{ArgAction, Parser, ValueEnum};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "globe",
    version,
    about = "Rotating Earth in your terminal — live sunlight, half-blocks, 256 colors",
    disable_help_flag = true
)]
pub struct Cli {
    /// Home position as "LAT,LON" in degrees. Default: derived from the system timezone.
    #[arg(short = 'h', long, value_name = "LAT,LON")]
    pub home: Option<String>,

    /// Frame rate cap (default 30, min 1, max 120).
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=120))]
    pub fps: u32,

    /// Render mode (default blocks).
    #[arg(long, value_enum, default_value_t = ModeArg::Blocks)]
    pub mode: ModeArg,

    /// Disable ANSI colors (same as --mode plain).
    #[arg(long, default_value_t = false)]
    pub no_color: bool,

    /// Snapshot mode: render one frame to stdout and exit.
    #[arg(long, default_value_t = false)]
    pub snapshot: bool,

    /// Cell-aspect ratio (cell height / cell width). Default 2.0 fits SF Mono,
    /// Menlo, and most modern monospace fonts — raise it if the globe looks
    /// vertically stretched.
    #[arg(long, default_value_t = 2.0)]
    pub cell_aspect: f64,

    /// Show help.
    #[arg(long, action = ArgAction::Help)]
    pub help: Option<bool>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeArg {
    Blocks,
    Ascii,
    Plain,
}

/// Effektiver Render-Modus aus `--mode` und `--no-color`.
pub fn effective_mode(mode: ModeArg, no_color: bool) -> ModeArg {
    if no_color {
        ModeArg::Plain
    } else {
        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("globe").chain(args.iter().copied()))
    }

    #[test]
    fn defaults() {
        let c = parse(&[]).unwrap();
        assert_eq!(c.home, None);
        assert_eq!(c.fps, 30);
        assert_eq!(c.mode, ModeArg::Blocks);
        assert!(!c.no_color);
        assert!(!c.snapshot);
    }

    #[test]
    fn short_home() {
        let c = parse(&["-h", "48.21,16.37"]).unwrap();
        assert_eq!(c.home.as_deref(), Some("48.21,16.37"));
    }

    #[test]
    fn long_home() {
        let c = parse(&["--home", "48.21,16.37"]).unwrap();
        assert_eq!(c.home.as_deref(), Some("48.21,16.37"));
    }

    #[test]
    fn mode_choices() {
        assert_eq!(parse(&["--mode", "blocks"]).unwrap().mode, ModeArg::Blocks);
        assert_eq!(parse(&["--mode", "ascii"]).unwrap().mode, ModeArg::Ascii);
        assert_eq!(parse(&["--mode", "plain"]).unwrap().mode, ModeArg::Plain);
    }

    #[test]
    fn unknown_mode_fails() {
        assert!(parse(&["--mode", "rainbow"]).is_err());
    }

    #[test]
    fn fps_range() {
        assert!(parse(&["--fps", "0"]).is_err());
        assert!(parse(&["--fps", "121"]).is_err());
        assert_eq!(parse(&["--fps", "60"]).unwrap().fps, 60);
    }

    #[test]
    fn no_color_overrides_mode() {
        assert_eq!(effective_mode(ModeArg::Blocks, true), ModeArg::Plain);
        assert_eq!(effective_mode(ModeArg::Ascii, true), ModeArg::Plain);
        assert_eq!(effective_mode(ModeArg::Plain, true), ModeArg::Plain);
        assert_eq!(effective_mode(ModeArg::Blocks, false), ModeArg::Blocks);
        assert_eq!(effective_mode(ModeArg::Ascii, false), ModeArg::Ascii);
    }

    #[test]
    fn snapshot_flag() {
        let c = parse(&["--snapshot"]).unwrap();
        assert!(c.snapshot);
    }
}
