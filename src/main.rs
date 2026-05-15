use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use clap::Parser;
use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{Write, stdout};
use std::time::{Duration, Instant};

use globe::app::{AppState, COARSE_LAT_STEP_DEG, COARSE_LON_STEP_DEG, RenderMode};
use globe::config::{Cli, ModeArg, effective_mode};
use globe::constants::{CELL_ASPECT_MAX, CELL_ASPECT_MIN, MIN_COLS, MIN_ROWS};
use globe::geo;
use globe::tui::FrameBuffer;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = geo::resolve_home(cli.home.as_deref())
        .map_err(|e| anyhow::anyhow!("--home: {}", e))?;
    let mode = match effective_mode(cli.mode, cli.no_color) {
        ModeArg::Blocks => RenderMode::Blocks,
        ModeArg::Ascii => RenderMode::Ascii,
        ModeArg::Plain => RenderMode::Plain,
    };

    let cell_aspect = cli.cell_aspect.clamp(CELL_ASPECT_MIN, CELL_ASPECT_MAX);
    let mut app = AppState::with_cell_aspect(home, mode, cell_aspect);

    if cli.snapshot {
        return run_snapshot(&app);
    }

    run_interactive(&mut app, cli.fps)
}

fn run_snapshot(app: &AppState) -> Result<()> {
    let mut fb = FrameBuffer::new(80, 40);
    let now = Utc
        .with_ymd_and_hms(2026, 3, 20, 12, 0, 0)
        .single()
        .context("snapshot fixed time")?;
    app.render(&mut fb, now);
    let mut out = stdout();
    fb.flush_diff(&mut out)?;
    writeln!(out)?;
    Ok(())
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn run_interactive(app: &mut AppState, fps: u32) -> Result<()> {
    let mut out = stdout();
    terminal::enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, cursor::Hide)?;
    let _guard = TerminalGuard;

    let active_frame_dur = Duration::from_millis((1000 / fps).max(1) as u64);
    let mut fb = FrameBuffer::new(0, 0);
    let mut last_step = Instant::now();

    loop {
        if event::poll(active_frame_dur)? {
            let ev = event::read()?;
            if let Event::Key(k) = ev {
                if k.kind == KeyEventKind::Press {
                    if dispatch_key(app, k.code, k.modifiers) {
                        return Ok(());
                    }
                }
            } else if let Event::Resize(_, _) = ev {
                fb.force_full_redraw();
            }
        }

        let (cols, rows) = terminal::size()?;
        if cols < MIN_COLS || rows < MIN_ROWS {
            show_too_small(&mut out, cols, rows)?;
            continue;
        }
        fb.resize(cols as usize, rows as usize);

        let now_t = Instant::now();
        let dt = now_t.duration_since(last_step);
        last_step = now_t;
        app.step(dt);

        app.render(&mut fb, Utc::now());
        fb.flush_diff(&mut out)?;
    }
}

/// Returns true if user wants to quit.
fn dispatch_key(app: &mut AppState, code: KeyCode, mods: KeyModifiers) -> bool {
    let fine = mods.contains(KeyModifiers::SHIFT);
    let now = Utc::now();
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Left => app.handle_arrow(-COARSE_LON_STEP_DEG, 0.0, fine),
        KeyCode::Right => app.handle_arrow(COARSE_LON_STEP_DEG, 0.0, fine),
        KeyCode::Up => app.handle_arrow(0.0, COARSE_LAT_STEP_DEG, fine),
        KeyCode::Down => app.handle_arrow(0.0, -COARSE_LAT_STEP_DEG, fine),
        KeyCode::Char('+') | KeyCode::Char('=') => app.handle_zoom_in(),
        KeyCode::Char('-') | KeyCode::Char('_') => app.handle_zoom_out(),
        KeyCode::Char('0') => app.handle_zoom_reset(),
        KeyCode::Char('r') => app.handle_reset(),
        KeyCode::Char('h') => app.handle_home(),
        KeyCode::Char('s') => app.handle_subsolar(now),
        KeyCode::Char('f') => app.handle_freeze(now),
        KeyCode::Char(' ') => app.handle_rotation_toggle(),
        KeyCode::Char(',') | KeyCode::Char('<') | KeyCode::Char('[') => app.handle_speed_down(),
        KeyCode::Char('.') | KeyCode::Char('>') | KeyCode::Char(']') => app.handle_speed_up(),
        KeyCode::Char('m') => app.handle_mode_cycle(),
        KeyCode::Char('c') => app.handle_clouds_toggle(),
        KeyCode::Char('e') => app.handle_equator_toggle(),
        KeyCode::Char('g') => app.handle_meridian_toggle(),
        KeyCode::Char(')') => app.handle_cell_aspect_inc(),
        KeyCode::Char('(') => app.handle_cell_aspect_dec(),
        KeyCode::Char('?') => app.handle_help_toggle(),
        _ => {}
    }
    false
}

fn show_too_small<W: Write>(out: &mut W, cols: u16, rows: u16) -> Result<()> {
    write!(
        out,
        "\x1b[H\x1b[2JTerminal too small ({}x{}, need >={}x{})",
        cols, rows, MIN_COLS, MIN_ROWS
    )?;
    out.flush()?;
    Ok(())
}
