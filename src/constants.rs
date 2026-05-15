use std::f64::consts::PI;

pub const FOV_HALF: f64 = 22.5 * PI / 180.0;

pub const ZOOM_MIN: f64 = 1.05;
pub const ZOOM_DEFAULT: f64 = 3.0;
pub const ZOOM_MAX: f64 = 30.0;
pub const ZOOM_STEP_IN: f64 = 0.85;
pub const ZOOM_STEP_OUT: f64 = 1.18;

pub const ROT_SECONDS_PER_TURN_REALTIME: f64 = 86_400.0;
pub const ROT_SPEED_STEPS: [f64; 7] = [1.0, 10.0, 100.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0];

pub const SUN_MOON_VISIBLE_DIST: f64 = 2.0;
pub const GLOW_OUTER: f64 = 1.06;
pub const CLOUD_RADIUS: f64 = 1.005;
pub const STAR_DENSITY: f64 = 0.0015;

pub const TARGET_FPS_ACTIVE: u32 = 30;
pub const IDLE_FPS: u32 = 1;

pub const HOME_FALLBACK_LAT: f64 = 0.0;
pub const HOME_FALLBACK_LON: f64 = 0.0;
pub const HOME_DEFAULT_LAT_NORTH: f64 = 45.0;

pub const MIN_COLS: u16 = 20;
pub const MIN_ROWS: u16 = 10;

/// Cell-Höhe in Cell-Breiten. Standard ist 2.0 (jede Terminal-Zelle ist genau
/// doppelt so hoch wie breit). Bei realen Fonts (SF Mono, Menlo) liegt der
/// Wert oft bei 2.05–2.15 — was im ASCII-Globus zu einer leichten vertikalen
/// Streckung führt. Live justierbar mit `(` und `)`.
pub const CELL_ASPECT_DEFAULT: f64 = 2.0;
pub const CELL_ASPECT_MIN: f64 = 1.4;
pub const CELL_ASPECT_MAX: f64 = 3.0;
pub const CELL_ASPECT_STEP: f64 = 0.05;
