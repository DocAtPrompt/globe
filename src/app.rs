//! App-State, Event-Handler und Frame-Renderer.

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::camera::Camera;
use crate::constants::{
    CELL_ASPECT_DEFAULT, CELL_ASPECT_MAX, CELL_ASPECT_MIN, CELL_ASPECT_STEP, CLOUD_RADIUS,
    ROT_SECONDS_PER_TURN_REALTIME, ROT_SPEED_STEPS,
};
use crate::render::{
    self, class_color, glow_intensity, hit_to_geo, lighting, lights_visible, palette_day,
    ray_at_screen, ray_perp_to_origin, ray_sphere, rgb_to_ansi, star_at, sun_marker, Ray, Star,
    CITY_LIGHT_RGB,
};
use crate::sun;
use crate::tui::{Cell, FrameBuffer};
use crate::vec3::{from_lat_lon, rotate_y, V3};
use crate::world;

pub const FINE_FACTOR: f64 = 0.1;
pub const COARSE_LAT_STEP_DEG: f64 = 5.0;
pub const COARSE_LON_STEP_DEG: f64 = 5.0;

const RAMP: &[char] = &[
    ' ', '.', '\'', ':', ';', ',', '-', '~', '!', '=', '+', '*', 'o', '#', '%', '&', '@',
];
/// Stretches the Lambert distribution across the full ramp. Values > 1 push
/// midtones darker, < 1 push them brighter. 1.0 = linear. Empirically 0.8 keeps
/// the day side comfortably bright while preserving terminator detail.
const ASCII_GAMMA: f64 = 0.8;

/// Albedo-Multiplikator pro Klasse — sorgt im ASCII/Plain-Modus dafür, dass
/// Kontinent-Strukturen über die Helligkeit erkennbar sind (Wasser dunkler,
/// Land heller, Eis am hellsten). Bei farbigen Modi tragen die Klassenfarben
/// die Information; der Faktor verstärkt zusätzlich die Konturen.
fn class_albedo(c: crate::world::Class) -> f64 {
    use crate::world::Class;
    // Klassen-Differenzierung bleibt erhalten (Wasser dunkler als Land), aber
    // alle Werte sind angehoben, damit die ASCII-Welt insgesamt heller wirkt.
    match c {
        Class::DeepSea => 0.60,
        Class::Sea => 0.75,
        Class::Flatland => 0.95,
        Class::Upland => 1.00,
        Class::Mountain => 1.05,
        Class::Ice => 1.15,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Blocks,
    Ascii,
    Plain,
}

impl RenderMode {
    pub fn cycle(self) -> RenderMode {
        match self {
            RenderMode::Blocks => RenderMode::Ascii,
            RenderMode::Ascii => RenderMode::Plain,
            RenderMode::Plain => RenderMode::Blocks,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Blocks => "blocks",
            RenderMode::Ascii => "ascii",
            RenderMode::Plain => "plain",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoRotation {
    Off,
    On { speed_idx: usize },
}

impl AutoRotation {
    pub fn speed_factor(self) -> f64 {
        match self {
            AutoRotation::Off => 0.0,
            AutoRotation::On { speed_idx } => ROT_SPEED_STEPS[speed_idx],
        }
    }
}

pub struct AppState {
    pub camera: Camera,
    pub home: (f64, f64),
    pub mode: RenderMode,
    pub auto_rotation: AutoRotation,
    pub freeze: bool,
    pub help_visible: bool,
    pub clouds_visible: bool,
    pub equator_visible: bool,
    pub meridian_visible: bool,
    /// Verhältnis cell_height / cell_width. Standard 2.0, justierbar
    /// damit die Sphere bei real anders proportionierten Fonts rund bleibt.
    pub cell_aspect: f64,
    pub earth_rotation_rad: f64,
    /// Bei aktivem Freeze: Sun-/Moon-Anker-Zeit.
    pub freeze_anchor: Option<DateTime<Utc>>,
    /// Bei aktivem Freeze: Lat/Lon-Offset zur Subsolar-Position.
    pub sun_delta_deg: (f64, f64),
    /// Rotation-State, der beim Freeze-Beginn gespeichert wird.
    pre_freeze_rotation: Option<AutoRotation>,
}

impl AppState {
    pub fn new(home: (f64, f64), mode: RenderMode) -> Self {
        Self::with_cell_aspect(home, mode, CELL_ASPECT_DEFAULT)
    }

    pub fn with_cell_aspect(home: (f64, f64), mode: RenderMode, cell_aspect: f64) -> Self {
        Self {
            camera: Camera::new(home.0, home.1),
            home,
            mode,
            auto_rotation: AutoRotation::On { speed_idx: 0 },
            freeze: false,
            help_visible: false,
            clouds_visible: true,
            equator_visible: false,
            meridian_visible: false,
            cell_aspect: cell_aspect.clamp(CELL_ASPECT_MIN, CELL_ASPECT_MAX),
            earth_rotation_rad: 0.0,
            freeze_anchor: None,
            sun_delta_deg: (0.0, 0.0),
            pre_freeze_rotation: None,
        }
    }

    // ----- Time -----------------------------------------------------------

    /// True wenn das Tool gerade keine visuelle Bewegung pro Frame liefert.
    /// Im Idle drosselt der Main-Loop die Frame-Rate, um CPU zu sparen — der
    /// Subsolar-Punkt wandert in Realtime nur 0.004°/s, also reicht 1 fps.
    pub fn is_idle(&self) -> bool {
        if self.freeze {
            return true;
        }
        match self.auto_rotation {
            AutoRotation::Off => true,
            AutoRotation::On { speed_idx: 0 } => true,
            AutoRotation::On { .. } => false,
        }
    }

    pub fn step(&mut self, dt: Duration) {
        if self.freeze {
            return;
        }
        let factor = self.auto_rotation.speed_factor();
        if factor == 0.0 {
            return;
        }
        let rad_per_sec = factor * 2.0 * PI / ROT_SECONDS_PER_TURN_REALTIME;
        self.earth_rotation_rad += rad_per_sec * dt.as_secs_f64();
        self.earth_rotation_rad = self.earth_rotation_rad.rem_euclid(2.0 * PI);
    }

    pub fn effective_sun_dir(&self, now: DateTime<Utc>) -> V3 {
        let base = self.freeze_anchor.unwrap_or(now);
        let (lat_deg, lon_deg) = sun::subsolar_point(base);
        let lat = (lat_deg + self.sun_delta_deg.0).clamp(-90.0, 90.0);
        let lon = lon_deg + self.sun_delta_deg.1;
        // subsolar_point liefert den Subsolar im *Erd-fixed* Frame (enthält GMST).
        // Für Lighting brauchen wir den Vektor im *Welt-Frame* — daher um die
        // bisher akkumulierte Erddrehung zurückrotieren. Ohne das wandert die
        // Tag/Nacht-Grenze doppelt so schnell (Earth dreht UND sub_lon kriecht).
        let dir_earth = from_lat_lon(lat.to_radians(), lon.to_radians());
        rotate_y(dir_earth, self.earth_rotation_rad)
    }

    pub fn effective_now(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.freeze_anchor.unwrap_or(now)
    }

    // ----- Key Handlers ---------------------------------------------------

    pub fn handle_arrow(&mut self, dx_deg: f64, dy_deg: f64, fine: bool) {
        let f = if fine { FINE_FACTOR } else { 1.0 };
        if self.freeze {
            let mut lat = self.sun_delta_deg.0 + dy_deg * f;
            let mut lon = self.sun_delta_deg.1 + dx_deg * f;
            lat = lat.clamp(-90.0, 90.0);
            // Wrap lon to [-180, 180]
            lon = ((lon + 180.0).rem_euclid(360.0)) - 180.0;
            self.sun_delta_deg = (lat, lon);
        } else {
            self.camera.rotate(dy_deg * f, dx_deg * f);
        }
    }

    pub fn handle_zoom_in(&mut self) {
        self.camera.zoom_in();
    }
    pub fn handle_zoom_out(&mut self) {
        self.camera.zoom_out();
    }
    pub fn handle_zoom_reset(&mut self) {
        self.camera.zoom_reset();
    }

    pub fn handle_home(&mut self) {
        self.camera.jump_to(self.home.0, self.home.1);
    }

    pub fn handle_subsolar(&mut self, now: DateTime<Utc>) {
        let base = self.freeze_anchor.unwrap_or(now);
        let (_lat, sub_lon) = sun::subsolar_point(base);
        self.camera.jump_to(0.0, sub_lon);
    }

    pub fn handle_freeze(&mut self, now: DateTime<Utc>) {
        if self.freeze {
            // Beenden: Delta verwerfen, Auto-Rotation wiederherstellen
            self.freeze = false;
            self.freeze_anchor = None;
            self.sun_delta_deg = (0.0, 0.0);
            if let Some(prev) = self.pre_freeze_rotation.take() {
                self.auto_rotation = prev;
            }
        } else {
            self.freeze = true;
            self.freeze_anchor = Some(now);
            self.pre_freeze_rotation = Some(self.auto_rotation);
            self.auto_rotation = AutoRotation::Off;
        }
    }

    pub fn handle_reset(&mut self) {
        self.camera.zoom_reset();
        self.auto_rotation = AutoRotation::On { speed_idx: 0 };
        self.sun_delta_deg = (0.0, 0.0);
        self.freeze = false;
        self.freeze_anchor = None;
        self.pre_freeze_rotation = None;
        self.mode = RenderMode::Blocks;
        self.clouds_visible = true;
        self.equator_visible = false;
        self.meridian_visible = false;
        self.help_visible = false;
    }

    pub fn handle_rotation_toggle(&mut self) {
        if matches!(self.auto_rotation, AutoRotation::On { .. }) {
            self.auto_rotation = AutoRotation::Off;
        } else {
            self.auto_rotation = AutoRotation::On { speed_idx: 0 };
        }
    }

    pub fn handle_speed_up(&mut self) {
        if let AutoRotation::On { speed_idx } = self.auto_rotation {
            let new_idx = (speed_idx + 1).min(ROT_SPEED_STEPS.len() - 1);
            self.auto_rotation = AutoRotation::On { speed_idx: new_idx };
        } else {
            self.auto_rotation = AutoRotation::On { speed_idx: 0 };
        }
    }

    pub fn handle_speed_down(&mut self) {
        if let AutoRotation::On { speed_idx } = self.auto_rotation {
            let new_idx = speed_idx.saturating_sub(1);
            self.auto_rotation = AutoRotation::On { speed_idx: new_idx };
        }
    }

    pub fn handle_mode_cycle(&mut self) {
        self.mode = self.mode.cycle();
    }

    pub fn handle_help_toggle(&mut self) {
        self.help_visible = !self.help_visible;
    }

    pub fn handle_clouds_toggle(&mut self) {
        self.clouds_visible = !self.clouds_visible;
    }

    pub fn handle_equator_toggle(&mut self) {
        self.equator_visible = !self.equator_visible;
    }

    pub fn handle_meridian_toggle(&mut self) {
        self.meridian_visible = !self.meridian_visible;
    }

    pub fn handle_cell_aspect_inc(&mut self) {
        self.cell_aspect = (self.cell_aspect + CELL_ASPECT_STEP).min(CELL_ASPECT_MAX);
    }

    pub fn handle_cell_aspect_dec(&mut self) {
        self.cell_aspect = (self.cell_aspect - CELL_ASPECT_STEP).max(CELL_ASPECT_MIN);
    }

    // ----- Rendering ------------------------------------------------------

    pub fn render(&self, fb: &mut FrameBuffer, now: DateTime<Utc>) {
        let cols = fb.cols();
        let total_rows = fb.rows();
        if cols == 0 || total_rows == 0 {
            return;
        }
        // Status-Zeile ist die letzte
        let render_rows = total_rows.saturating_sub(1);
        if render_rows == 0 {
            return;
        }

        // Vorher Render-Fläche löschen
        for y in 0..render_rows {
            for x in 0..cols {
                fb.put(x, y, Cell::EMPTY);
            }
        }

        let sun_dir = self.effective_sun_dir(now);
        let base_now = self.effective_now(now);

        match self.mode {
            RenderMode::Blocks => {
                self.render_blocks(fb, cols, render_rows, sun_dir);
            }
            RenderMode::Ascii => {
                self.render_ascii(fb, cols, render_rows, sun_dir, true);
            }
            RenderMode::Plain => {
                self.render_ascii(fb, cols, render_rows, sun_dir, false);
            }
        }

        // Sonne / Mond Marker — Sub-Höhe konsistent mit Render-Funktionen
        let sub_h = render_rows as f64 * self.cell_aspect;
        if let Some((sx, sy)) = sun_marker(
            base_now,
            self.earth_rotation_rad,
            &self.camera,
            cols as f64,
            sub_h,
        ) {
            self.place_sun(fb, sx, sy, render_rows);
        }
        if let Some(((sx, sy), illum)) = render::moon_marker(
            base_now,
            self.earth_rotation_rad,
            &self.camera,
            cols as f64,
            sub_h,
        ) {
            self.place_moon(fb, sx, sy, render_rows, illum);
        }

        // Status-Zeile
        let status = self.format_status();
        self.draw_status(fb, &status, total_rows - 1);

        // Help-Overlay zuletzt (über alles)
        if self.help_visible {
            self.draw_help(fb);
        }
    }

    fn render_blocks(&self, fb: &mut FrameBuffer, cols: usize, render_rows: usize, sun_dir: V3) {
        // Welt-Y-Höhe einer Zelle = `cell_aspect` wide-units. Zwei Halbblock-
        // Sub-Pixel pro Zelle, jeweils gesampelt in deren Mitte.
        let sub_h = render_rows as f64 * self.cell_aspect;
        let half = self.cell_aspect * 0.5;
        let quarter = self.cell_aspect * 0.25;
        let w = cols as f64;
        for y in 0..render_rows {
            let y0 = y as f64 * self.cell_aspect + quarter;
            let y1 = y0 + half;
            for x in 0..cols {
                let xc = x as f64 + 0.5;
                let ray_up = ray_at_screen(xc, y0, w, sub_h, &self.camera);
                let ray_dn = ray_at_screen(xc, y1, w, sub_h, &self.camera);
                let fg = shade_color(
                    &ray_up,
                    sun_dir,
                    self.earth_rotation_rad,
                    x as u32,
                    (y * 2) as u32,
                    self.camera.distance,
                    self.clouds_visible,
                    self.equator_visible,
                    self.meridian_visible,
                );
                let bg = shade_color(
                    &ray_dn,
                    sun_dir,
                    self.earth_rotation_rad,
                    x as u32,
                    (y * 2 + 1) as u32,
                    self.camera.distance,
                    self.clouds_visible,
                    self.equator_visible,
                    self.meridian_visible,
                );
                fb.put(x, y, Cell::new('▀', fg, bg));
            }
        }
    }

    fn render_ascii(
        &self,
        fb: &mut FrameBuffer,
        cols: usize,
        render_rows: usize,
        sun_dir: V3,
        color: bool,
    ) {
        // Sub-Höhe = render_rows * cell_aspect; eine Zelle ist `cell_aspect`
        // wide-units hoch, wir sampeln in ihrer Mitte.
        let w = cols as f64;
        let sub_h = render_rows as f64 * self.cell_aspect;
        for y in 0..render_rows {
            let y_sample = (y as f64) * self.cell_aspect + self.cell_aspect * 0.5;
            for x in 0..cols {
                let ray = ray_at_screen(x as f64 + 0.5, y_sample, w, sub_h, &self.camera);
                let (ch, fg, bg) = shade_ascii(
                    &ray,
                    sun_dir,
                    self.earth_rotation_rad,
                    x as u32,
                    y as u32,
                    self.camera.distance,
                    color,
                    self.equator_visible,
                    self.meridian_visible,
                );
                fb.put(x, y, Cell::new(ch, if color { fg } else { 15 }, bg));
            }
        }
    }

    fn put_marker_cell(
        &self,
        fb: &mut FrameBuffer,
        x: i64,
        y: i64,
        render_rows: usize,
        ch: char,
        fg: u8,
    ) {
        if x < 0 || x as usize >= fb.cols() {
            return;
        }
        if y < 0 || y as usize >= render_rows {
            return;
        }
        let bg = fb.get(x as usize, y as usize).bg;
        fb.put(x as usize, y as usize, Cell::new(ch, fg, bg));
    }

    fn place_sun(&self, fb: &mut FrameBuffer, sx: f64, sy: f64, render_rows: usize) {
        let x = sx.floor() as i64;
        // Sub-Pixel-Y in Zellzeile mappen — wir teilen durch cell_aspect.
        let y = (sy / self.cell_aspect).floor() as i64;
        // Stern: Center + horizontale + vertikale Strahlen
        self.put_marker_cell(fb, x, y, render_rows, '●', 226);
        self.put_marker_cell(fb, x - 1, y, render_rows, '*', 220);
        self.put_marker_cell(fb, x + 1, y, render_rows, '*', 220);
        self.put_marker_cell(fb, x, y - 1, render_rows, '*', 220);
        self.put_marker_cell(fb, x, y + 1, render_rows, '*', 220);
    }

    fn place_moon(&self, fb: &mut FrameBuffer, sx: f64, sy: f64, render_rows: usize, illum: f64) {
        let x = sx.floor() as i64;
        let y = (sy / self.cell_aspect).floor() as i64;
        let ch = moon_phase_char(illum);
        // 3-Zellen breiter Marker: zentrales Phase-Symbol + dezente Mond-Schatten
        // links/rechts. So ist der Mond deutlich auffälliger ohne ihn massiv zu machen.
        self.put_marker_cell(fb, x - 1, y, render_rows, '·', 244);
        self.put_marker_cell(fb, x, y, render_rows, ch, 255);
        self.put_marker_cell(fb, x + 1, y, render_rows, '·', 244);
    }

    fn format_status(&self) -> String {
        let mut s = String::with_capacity(160);
        let _ = write!(
            s,
            "lat {:+.1}° lon {:+.1}° | zoom {:.2}",
            self.camera.lat_deg, self.camera.lon_deg, self.camera.distance
        );
        // Mondphase: aktuell immer, weil sie als Live-Info sinnvoll ist
        let base_now = self.freeze_anchor.unwrap_or_else(Utc::now);
        let illum = crate::moon::illumination(base_now);
        // Phasen-Richtung: 6h später hat sich der Beleuchtungsanteil messbar
        // verschoben (Mondzyklus ~29.5 Tage → ~1.4 %-Punkte pro 6h im Mittel).
        let illum_later = crate::moon::illumination(base_now + chrono::Duration::hours(6));
        let direction = if illum_later > illum + 0.003 {
            " ↑"
        } else if illum_later < illum - 0.003 {
            " ↓"
        } else {
            ""
        };
        let _ = write!(
            s,
            " | moon: {}{} {:.0}%",
            moon_phase_label(illum),
            direction,
            (illum * 100.0).round()
        );
        if self.freeze {
            let _ = write!(
                s,
                " | sun Δ{:+.0}°,{:+.0}° | FREEZE",
                self.sun_delta_deg.0, self.sun_delta_deg.1
            );
        } else {
            let rot_txt = match self.auto_rotation {
                AutoRotation::Off => "off".to_string(),
                AutoRotation::On { speed_idx } => {
                    format!("{}×", ROT_SPEED_STEPS[speed_idx] as u64)
                }
            };
            let _ = write!(s, " | sun: live | rot: {}", rot_txt);
        }
        let cl = if self.clouds_visible { "on" } else { "off" };
        let _ = write!(
            s,
            " | mode: {} | clouds: {} | aspect: {:.2}  [?] help",
            self.mode.label(),
            cl,
            self.cell_aspect
        );
        s
    }

    fn draw_status(&self, fb: &mut FrameBuffer, status: &str, row: usize) {
        let cols = fb.cols();
        let mut x = 0;
        for ch in status.chars() {
            if x >= cols {
                break;
            }
            fb.put(x, row, Cell::new(ch, 248, 16));
            x += 1;
        }
        // Rest auffüllen
        while x < cols {
            fb.put(x, row, Cell::new(' ', 248, 16));
            x += 1;
        }
    }

    fn draw_help(&self, fb: &mut FrameBuffer) {
        let lines = [
            "globe — keys",
            "",
            "  ←→↑↓        rotate earth (lat/lon)",
            "  Shift+arrow fine rotation (10× smaller step)",
            "  + −         zoom in/out",
            "  0           reset zoom",
            "  h           jump to home position",
            "  s           jump to subsolar position",
            "  f           freeze toggle (arrows move the sun)",
            "  Space       toggle auto-rotation",
            "  , .         rotation speed −/+",
            "  m           cycle mode blocks/ascii/plain",
            "  c           toggle cloud layer",
            "  e           toggle equator line",
            "  g           toggle Greenwich meridian",
            "  ( )         adjust cell-aspect",
            "  r           restore defaults (position kept)",
            "  ?           toggle this help",
            "  q / Esc     quit",
        ];
        let cols = fb.cols();
        let rows = fb.rows();
        let h = lines.len() + 2;
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) + 4;
        if h > rows || w > cols {
            return;
        }
        let x0 = (cols - w) / 2;
        let y0 = (rows - h) / 2;
        // Border
        for dy in 0..h {
            for dx in 0..w {
                let ch = if dy == 0 || dy == h - 1 {
                    '─'
                } else if dx == 0 || dx == w - 1 {
                    '│'
                } else {
                    ' '
                };
                fb.put(x0 + dx, y0 + dy, Cell::new(ch, 252, 234));
            }
        }
        fb.put(x0, y0, Cell::new('┌', 252, 234));
        fb.put(x0 + w - 1, y0, Cell::new('┐', 252, 234));
        fb.put(x0, y0 + h - 1, Cell::new('└', 252, 234));
        fb.put(x0 + w - 1, y0 + h - 1, Cell::new('┘', 252, 234));
        // Lines
        for (i, line) in lines.iter().enumerate() {
            let lx = x0 + 2;
            let ly = y0 + 1 + i;
            for (j, ch) in line.chars().enumerate() {
                if lx + j >= cols {
                    break;
                }
                fb.put(lx + j, ly, Cell::new(ch, 252, 234));
            }
        }
    }
}

// ----- Shading helpers (free functions) ------------------------------------

/// Lat-Schwelle für die Äquator-Linie. 0.012 entspricht ≈ 0.69°
/// (auf der Sphere ist y = sin(lat), bei kleinem Lat näherungsweise identisch).
/// Untere Grenze ergibt sich aus der Sub-Pixel-Pitch: bei Camera lat=0
/// liegen Sample-Centers oberhalb/unterhalb der Linie um ≈ 0.01 — schmaler
/// als das verliert man die Linie zwischen den Pixeln.
const EQUATOR_HALF_WIDTH: f64 = 0.012;
/// Lon-Schwelle für den Greenwich-Meridian in Radian.
const MERIDIAN_HALF_WIDTH: f64 = 0.012;
/// Maximale Beimischung der Linien-Farbe an der dichtesten Stelle.
/// Niedriger Wert + softer Fade ergibt eine optisch dünne Linie, die aber
/// immer auf min. einen Pixel trifft.
const LINE_PEAK_ALPHA: f64 = 0.55;

#[allow(clippy::too_many_arguments)]
fn shade_color(
    ray: &Ray,
    sun_dir: V3,
    earth_rot: f64,
    pix_x: u32,
    pix_y: u32,
    cam_distance: f64,
    clouds_visible: bool,
    equator_visible: bool,
    meridian_visible: bool,
) -> u8 {
    let earth = ray_sphere(ray, 1.0);
    if let Some(hit) = earth {
        let (lat, lon) = hit_to_geo(hit.point, earth_rot);
        let class = world::sample_class(lat, lon);
        let light = lighting(hit.point, sun_dir);

        // Cloud vor Erde? Nur prüfen wenn Wolken eingeblendet sind.
        let final_rgb = if clouds_visible {
            let cloud = ray_sphere(ray, CLOUD_RADIUS);
            if let Some(ch) = cloud {
                if ch.t < hit.t {
                    let (clat, clon) = hit_to_geo(ch.point, earth_rot * 1.2);
                    let alpha = world::sample_clouds(clat, clon) as f64 / 255.0;
                    if alpha > 0.0 {
                        let base = class_color(class, light);
                        let cloud_lit = (210.0 * (0.2 + 0.8 * light)).clamp(20.0, 255.0) as u8;
                        let cloud_rgb = (cloud_lit, cloud_lit, cloud_lit);
                        mix_rgb_u8(base, cloud_rgb, alpha)
                    } else {
                        class_color(class, light)
                    }
                } else {
                    class_color(class, light)
                }
            } else {
                class_color(class, light)
            }
        } else {
            class_color(class, light)
        };

        // Stadtlichter (nur Nachtseite)
        if light < 0.05 {
            let strength = world::sample_lights(lat, lon);
            if lights_visible(strength, cam_distance) {
                return rgb_to_ansi(CITY_LIGHT_RGB.0, CITY_LIGHT_RGB.1, CITY_LIGHT_RGB.2);
            }
        }
        // Geo-Linien-Overlay mit weichen Rändern. Threshold breit genug, dass
        // mindestens ein Sub-Pixel pro Längen-/Breitenrad getroffen wird;
        // niedrige Peak-Alpha macht die Linie trotzdem optisch dünn.
        let mut out = final_rgb;
        if equator_visible {
            let d = hit.point.1.abs();
            if d < EQUATOR_HALF_WIDTH {
                let intensity = 1.0 - d / EQUATOR_HALF_WIDTH;
                out = mix_rgb_u8(out, (255, 220, 60), LINE_PEAK_ALPHA * intensity);
            }
        }
        if meridian_visible {
            let d = lon.abs();
            if d < MERIDIAN_HALF_WIDTH {
                let intensity = 1.0 - d / MERIDIAN_HALF_WIDTH;
                out = mix_rgb_u8(out, (110, 200, 255), LINE_PEAK_ALPHA * intensity);
            }
        }
        rgb_to_ansi(out.0, out.1, out.2)
    } else {
        let perp = ray_perp_to_origin(ray);
        let g = glow_intensity(perp);
        if g > 0.0 {
            // Tangentialpunkt: dort wo der Strahl der Sphere am nächsten kommt.
            // Sein "Light"-Wert sagt uns, ob wir auf der Tag- oder Nachtseite
            // der Erde sind — entsprechend warmer Sonnenuntergangs-Glow oder
            // kalter Nachtseite-Saum.
            let t_close = -ray.origin.dot(ray.dir);
            let tangent = ray.origin.add(ray.dir.mul(t_close));
            let light = lighting(tangent.norm(), sun_dir);
            let (r, gn, b) = if light > 0.5 {
                // voll auf Tagseite: hell-warm, neutral
                (200.0, 200.0, 150.0)
            } else if light > 0.15 {
                // Sonnenauf-/-untergang: warm beige (weniger aggressiv)
                (200.0, 170.0, 110.0)
            } else if light > 0.02 {
                // Dämmerungssaum: kühl-lila statt rosa
                (120.0, 110.0, 140.0)
            } else {
                // Nachtseite: kaltes Blau (klassischer Atmosphären-Saum)
                (30.0, 70.0, 150.0)
            };
            return rgb_to_ansi((r * g) as u8, (gn * g) as u8, (b * g) as u8);
        }
        match star_at(pix_x, pix_y) {
            Some(Star::Bright) => return rgb_to_ansi(255, 255, 240),
            Some(Star::Medium) => return rgb_to_ansi(180, 180, 180),
            Some(Star::Dim) => return rgb_to_ansi(110, 110, 130),
            None => {}
        }
        16
    }
}

#[allow(clippy::too_many_arguments)]
fn shade_ascii(
    ray: &Ray,
    sun_dir: V3,
    earth_rot: f64,
    pix_x: u32,
    pix_y: u32,
    cam_distance: f64,
    color: bool,
    equator_visible: bool,
    meridian_visible: bool,
) -> (char, u8, u8) {
    let earth = ray_sphere(ray, 1.0);
    if let Some(hit) = earth {
        let (lat, lon) = hit_to_geo(hit.point, earth_rot);
        let class = world::sample_class(lat, lon);
        // Lambert-Diffuse (max(0, n·s)) — sanfter Verlauf von Tagseite zu Nacht.
        let raw = hit.point.dot(sun_dir);
        let night_side = raw < 0.05;

        if night_side {
            let strength = world::sample_lights(lat, lon);
            if lights_visible(strength, cam_distance) {
                let fg = if color {
                    rgb_to_ansi(CITY_LIGHT_RGB.0, CITY_LIGHT_RGB.1, CITY_LIGHT_RGB.2)
                } else {
                    15
                };
                return ('·', fg, 16);
            }
            return (' ', 16, 16);
        }

        // Gamma-Stretch + Klassen-Albedo: ohne diese beiden Korrekturen sehen alle
        // Pixel der Tagseite gleich hell aus und Kontinent-Konturen verschwinden.
        let lambert = raw.max(0.0).powf(ASCII_GAMMA);
        let visual = (lambert * class_albedo(class)).clamp(0.0, 1.0);
        let idx = ((visual * (RAMP.len() as f64 - 0.01)) as usize).min(RAMP.len() - 1);
        let ch = RAMP[idx];
        // Geo-Linien: weicher Farb-Blend statt hartem Char-Override (war im
        // Vergleich zum Blocks-Modus deutlich aufdringlicher als beabsichtigt).
        let eq_intensity = if equator_visible {
            let d = hit.point.1.abs();
            if d < EQUATOR_HALF_WIDTH {
                LINE_PEAK_ALPHA * (1.0 - d / EQUATOR_HALF_WIDTH)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let mer_intensity = if meridian_visible {
            let d = lon.abs();
            if d < MERIDIAN_HALF_WIDTH {
                LINE_PEAK_ALPHA * (1.0 - d / MERIDIAN_HALF_WIDTH)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let (fg, bg) = if color {
            let day = palette_day(class);
            // Foreground: helle Variante der Klassenfarbe für das Char-Glyph.
            let bright = 0.6 + 0.4 * lambert;
            let mut fg_rgb = (
                (day.0 as f64 * bright).min(255.0) as u8,
                (day.1 as f64 * bright).min(255.0) as u8,
                (day.2 as f64 * bright).min(255.0) as u8,
            );
            // Background: gedämpfte Klassenfarbe — füllt die Zelle und liefert
            // den eigentlichen Farb-Eindruck. Lambert-skaliert, damit Nachtrand
            // dunkler wird.
            let dim = 0.25 + 0.35 * lambert;
            let mut bg_rgb = (
                (day.0 as f64 * dim) as u8,
                (day.1 as f64 * dim) as u8,
                (day.2 as f64 * dim) as u8,
            );
            if eq_intensity > 0.0 {
                fg_rgb = mix_rgb_u8(fg_rgb, (255, 220, 60), eq_intensity);
                bg_rgb = mix_rgb_u8(bg_rgb, (160, 130, 30), eq_intensity);
            }
            if mer_intensity > 0.0 {
                fg_rgb = mix_rgb_u8(fg_rgb, (110, 200, 255), mer_intensity);
                bg_rgb = mix_rgb_u8(bg_rgb, (50, 110, 160), mer_intensity);
            }
            (
                rgb_to_ansi(fg_rgb.0, fg_rgb.1, fg_rgb.2),
                rgb_to_ansi(bg_rgb.0, bg_rgb.1, bg_rgb.2),
            )
        } else {
            (15, 16)
        };
        (ch, fg, bg)
    } else {
        let perp = ray_perp_to_origin(ray);
        let gl = glow_intensity(perp);
        if gl > 0.0 {
            // Atmosphären-Saum: gleiche Tag/Nacht-Farb-Logik wie in Blocks.
            let t_close = -ray.origin.dot(ray.dir);
            let tangent = ray.origin.add(ray.dir.mul(t_close));
            let light = lighting(tangent.norm(), sun_dir);
            let (r, g, b) = if light > 0.5 {
                (200.0, 200.0, 150.0)
            } else if light > 0.15 {
                (200.0, 170.0, 110.0)
            } else if light > 0.02 {
                (120.0, 110.0, 140.0)
            } else {
                (30.0, 70.0, 150.0)
            };
            // Char: schwacher Punkt am inneren Halo, kompakter am Sphere-Rand.
            let ch = if gl > 0.6 {
                '·'
            } else if gl > 0.3 {
                '.'
            } else {
                '.'
            };
            return (
                ch,
                if color {
                    rgb_to_ansi((r * gl) as u8, (g * gl) as u8, (b * gl) as u8)
                } else {
                    15
                },
                16,
            );
        }
        match star_at(pix_x, pix_y) {
            Some(Star::Bright) => {
                return (
                    '*',
                    if color {
                        rgb_to_ansi(255, 255, 240)
                    } else {
                        15
                    },
                    16,
                );
            }
            Some(Star::Medium) => {
                return (
                    '.',
                    if color {
                        rgb_to_ansi(180, 180, 180)
                    } else {
                        15
                    },
                    16,
                );
            }
            Some(Star::Dim) => {
                return (
                    '·',
                    if color {
                        rgb_to_ansi(110, 110, 130)
                    } else {
                        15
                    },
                    16,
                );
            }
            None => {}
        }
        (' ', 16, 16)
    }
}

fn mix_rgb_u8(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| -> u8 {
        (x as f64 + (y as f64 - x as f64) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn moon_phase_char(illum: f64) -> char {
    // 0 = Neumond, 1 = Vollmond. Ohne Phasenrichtung — wir nehmen vereinfacht
    // einen Block, dessen Helligkeit der Phase entspricht.
    if illum < 0.05 {
        '○'
    } else if illum < 0.25 {
        '◗'
    } else if illum < 0.45 {
        '◐'
    } else if illum < 0.55 {
        '◑'
    } else if illum < 0.85 {
        '◔'
    } else {
        '●'
    }
}

fn moon_phase_label(illum: f64) -> &'static str {
    if illum < 0.05 {
        "New"
    } else if illum < 0.25 {
        "Crescent"
    } else if illum < 0.55 {
        "Quarter"
    } else if illum < 0.85 {
        "Gibbous"
    } else {
        "Full"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ZOOM_DEFAULT;
    use chrono::TimeZone;

    fn now_fixed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 15, 12, 0, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn new_sets_camera_to_home() {
        let app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        assert!((app.camera.lat_deg - 48.21).abs() < 1e-9);
        assert!((app.camera.lon_deg - 16.37).abs() < 1e-9);
        assert_eq!(app.camera.distance, ZOOM_DEFAULT);
        assert_eq!(app.mode, RenderMode::Blocks);
        assert!(matches!(
            app.auto_rotation,
            AutoRotation::On { speed_idx: 0 }
        ));
        assert!(!app.freeze);
    }

    #[test]
    fn step_advances_rotation_when_active() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        let r0 = app.earth_rotation_rad;
        app.step(Duration::from_secs(3600)); // 1h realtime → 15°
        let r1 = app.earth_rotation_rad;
        let degrees = (r1 - r0).to_degrees();
        assert!((degrees - 15.0).abs() < 0.5, "step 1h → {}°", degrees);
    }

    #[test]
    fn step_does_not_advance_in_freeze() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        app.handle_freeze(now_fixed());
        app.step(Duration::from_secs(3600));
        assert_eq!(app.earth_rotation_rad, 0.0);
    }

    #[test]
    fn arrow_in_normal_rotates_camera() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        app.handle_arrow(COARSE_LON_STEP_DEG, 0.0, false);
        assert!((app.camera.lon_deg - COARSE_LON_STEP_DEG).abs() < 1e-9);
    }

    #[test]
    fn arrow_in_freeze_adjusts_sun_delta() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        let cam_before = app.camera;
        app.handle_freeze(now_fixed());
        app.handle_arrow(10.0, 5.0, false);
        assert_eq!(app.camera, cam_before, "camera must not move in freeze");
        assert!((app.sun_delta_deg.1 - 10.0).abs() < 1e-9);
        assert!((app.sun_delta_deg.0 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn freeze_toggle_saves_and_restores_rotation() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        app.handle_speed_up(); // index 1
        let before = app.auto_rotation;
        app.handle_freeze(now_fixed());
        assert!(app.freeze);
        assert_eq!(app.auto_rotation, AutoRotation::Off);
        app.handle_freeze(now_fixed());
        assert!(!app.freeze);
        assert_eq!(app.auto_rotation, before);
        assert_eq!(app.sun_delta_deg, (0.0, 0.0));
    }

    #[test]
    fn subsolar_jump_sets_lat_zero() {
        let mut app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        app.handle_subsolar(now_fixed());
        assert!(app.camera.lat_deg.abs() < 1e-9);
    }

    #[test]
    fn home_jump_returns_to_home() {
        let mut app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        app.handle_arrow(50.0, 30.0, false);
        app.handle_home();
        assert!((app.camera.lat_deg - 48.21).abs() < 1e-9);
        assert!((app.camera.lon_deg - 16.37).abs() < 1e-9);
    }

    #[test]
    fn reset_keeps_camera_resets_others() {
        let mut app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        app.handle_arrow(20.0, 10.0, false);
        app.handle_zoom_in();
        app.handle_speed_up();
        app.handle_mode_cycle(); // → ascii
        let cam_before = app.camera;
        app.handle_reset();
        // distance reset on default, position keeps
        assert!((app.camera.lat_deg - cam_before.lat_deg).abs() < 1e-9);
        assert!((app.camera.lon_deg - cam_before.lon_deg).abs() < 1e-9);
        assert_eq!(app.camera.distance, ZOOM_DEFAULT);
        assert_eq!(app.mode, RenderMode::Blocks);
        assert_eq!(app.auto_rotation, AutoRotation::On { speed_idx: 0 });
    }

    #[test]
    fn mode_cycle_three_steps() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        app.handle_mode_cycle();
        assert_eq!(app.mode, RenderMode::Ascii);
        app.handle_mode_cycle();
        assert_eq!(app.mode, RenderMode::Plain);
        app.handle_mode_cycle();
        assert_eq!(app.mode, RenderMode::Blocks);
    }

    #[test]
    fn render_writes_some_non_empty_cells() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        app.handle_zoom_reset();
        let mut fb = FrameBuffer::new(40, 20);
        app.render(&mut fb, now_fixed());
        // Mindestens eine Zelle in der Render-Fläche darf nicht das Default-Leerzeichen sein
        let mut non_empty = 0;
        for y in 0..19 {
            for x in 0..40 {
                if fb.get(x, y) != Cell::EMPTY {
                    non_empty += 1;
                }
            }
        }
        assert!(non_empty > 100, "render filled only {} cells", non_empty);
    }

    #[test]
    fn render_status_line_in_last_row() {
        let app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        let mut fb = FrameBuffer::new(80, 20);
        app.render(&mut fb, now_fixed());
        let last = 19;
        // Status muss am Anfang der letzten Zeile irgendwo "lat" enthalten
        let s: String = (0..80).map(|x| fb.get(x, last).ch).collect();
        assert!(s.contains("lat "), "status row: {:?}", s);
    }

    #[test]
    fn speed_up_clamps_at_max() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        for _ in 0..50 {
            app.handle_speed_up();
        }
        if let AutoRotation::On { speed_idx } = app.auto_rotation {
            assert_eq!(speed_idx, ROT_SPEED_STEPS.len() - 1);
        } else {
            panic!("Auto-rotation should still be On");
        }
    }

    #[test]
    fn speed_down_clamps_at_min() {
        let mut app = AppState::new((0.0, 0.0), RenderMode::Blocks);
        for _ in 0..50 {
            app.handle_speed_down();
        }
        if let AutoRotation::On { speed_idx } = app.auto_rotation {
            assert_eq!(speed_idx, 0);
        } else {
            panic!("Auto-rotation should still be On");
        }
    }
}
