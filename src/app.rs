//! App-State, Event-Handler und Frame-Renderer.

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::camera::Camera;
use crate::constants::{
    CLOUD_RADIUS, ROT_SECONDS_PER_TURN_REALTIME, ROT_SPEED_STEPS,
};
use crate::render::{
    self, Ray, class_color, glow_intensity, hit_to_geo, lighting, lights_visible,
    palette_day, ray_for_sub_pixel, ray_perp_to_origin, ray_sphere, rgb_to_ansi,
    star_at, sun_marker,
};
use crate::sun;
use crate::tui::{Cell, FrameBuffer};
use crate::vec3::{V3, from_lat_lon};
use crate::world;

pub const FINE_FACTOR: f64 = 0.1;
pub const COARSE_LAT_STEP_DEG: f64 = 5.0;
pub const COARSE_LON_STEP_DEG: f64 = 5.0;

const RAMP: &[char] = &[
    ' ', '.', '\'', ':', ';', ',', '-', '~', '!', '=', '+', '*', 'o', '#', '%', '&', '@',
];
/// Spreizt die Lambert-Verteilung (große Tagseite-Fläche bei hohem cos θ) auf den
/// vollen Rampen-Bereich. Werte > 1 verschieben sichtbares Bild Richtung dunklere
/// Mitteltöne.
const ASCII_GAMMA: f64 = 1.6;

/// Albedo-Multiplikator pro Klasse — sorgt im ASCII/Plain-Modus dafür, dass
/// Kontinent-Strukturen über die Helligkeit erkennbar sind (Wasser dunkler,
/// Land heller, Eis am hellsten). Bei farbigen Modi tragen die Klassenfarben
/// die Information; der Faktor verstärkt zusätzlich die Konturen.
fn class_albedo(c: crate::world::Class) -> f64 {
    use crate::world::Class;
    match c {
        Class::DeepSea => 0.40,
        Class::Sea => 0.55,
        Class::Flatland => 0.85,
        Class::Upland => 0.95,
        Class::Mountain => 1.00,
        Class::Ice => 1.10,
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
        Self {
            camera: Camera::new(home.0, home.1),
            home,
            mode,
            auto_rotation: AutoRotation::On { speed_idx: 0 },
            freeze: false,
            help_visible: false,
            clouds_visible: true,
            earth_rotation_rad: 0.0,
            freeze_anchor: None,
            sun_delta_deg: (0.0, 0.0),
            pre_freeze_rotation: None,
        }
    }

    // ----- Time -----------------------------------------------------------

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
        from_lat_lon(lat.to_radians(), lon.to_radians())
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

        // Sonne / Mond Marker
        let sub_h = match self.mode {
            RenderMode::Blocks => render_rows as f64 * 2.0,
            _ => render_rows as f64 * 2.0,
        };
        if let Some((sx, sy)) = sun_marker(base_now, &self.camera, cols as f64, sub_h) {
            self.place_sun(fb, sx, sy, render_rows);
        }
        if let Some(((sx, sy), illum)) = render::moon_marker(base_now, &self.camera, cols as f64, sub_h) {
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

    fn render_blocks(
        &self,
        fb: &mut FrameBuffer,
        cols: usize,
        render_rows: usize,
        sun_dir: V3,
    ) {
        let sub_h = (render_rows * 2) as f64;
        let w = cols as f64;
        for y in 0..render_rows {
            for x in 0..cols {
                let ray_up = ray_for_sub_pixel(x as f64, (y * 2) as f64, w, sub_h, &self.camera);
                let ray_dn = ray_for_sub_pixel(x as f64, (y * 2 + 1) as f64, w, sub_h, &self.camera);
                let pix_x_up = x as u32;
                let pix_y_up = (y * 2) as u32;
                let pix_y_dn = (y * 2 + 1) as u32;
                let fg = shade_color(&ray_up, sun_dir, self.earth_rotation_rad, pix_x_up, pix_y_up, self.camera.distance, self.clouds_visible);
                let bg = shade_color(&ray_dn, sun_dir, self.earth_rotation_rad, pix_x_up, pix_y_dn, self.camera.distance, self.clouds_visible);
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
        // Terminal-Zellen sind ≈1:2 — also virtuelle Pixel-Höhe = render_rows * 2,
        // pro Zelle samplen wir genau in der vertikalen Mitte.
        let w = cols as f64;
        let sub_h = (render_rows as f64) * 2.0;
        for y in 0..render_rows {
            let y_sample = (y as f64) * 2.0 + 0.5;
            for x in 0..cols {
                let ray = ray_for_sub_pixel(x as f64, y_sample, w, sub_h, &self.camera);
                let (ch, fg) = shade_ascii(
                    &ray,
                    sun_dir,
                    self.earth_rotation_rad,
                    x as u32,
                    y as u32,
                    self.camera.distance,
                    color,
                );
                fb.put(x, y, Cell::new(ch, if color { fg } else { 15 }, 16));
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
        // Sub-Pixel-Y in Zellzeile mappen — wir teilen immer durch 2 (sub_h = rows*2 für alle Modi).
        let y = (sy / 2.0).floor() as i64;
        // Stern: Center + horizontale + vertikale Strahlen
        self.put_marker_cell(fb, x, y, render_rows, '●', 226);
        self.put_marker_cell(fb, x - 1, y, render_rows, '*', 220);
        self.put_marker_cell(fb, x + 1, y, render_rows, '*', 220);
        self.put_marker_cell(fb, x, y - 1, render_rows, '*', 220);
        self.put_marker_cell(fb, x, y + 1, render_rows, '*', 220);
    }

    fn place_moon(&self, fb: &mut FrameBuffer, sx: f64, sy: f64, render_rows: usize, illum: f64) {
        let x = sx.floor() as i64;
        let y = (sy / 2.0).floor() as i64;
        let ch = moon_phase_char(illum);
        self.put_marker_cell(fb, x, y, render_rows, ch, 255);
    }

    fn format_status(&self) -> String {
        let mut s = String::with_capacity(120);
        let _ = write!(
            s,
            "lat {:+.1}° lon {:+.1}° | zoom {:.2}",
            self.camera.lat_deg, self.camera.lon_deg, self.camera.distance
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
            " | mode: {} | clouds: {}  [?] help",
            self.mode.label(),
            cl
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
            "globe — Tastatur",
            "",
            "  ←→↑↓        Erde drehen (Lat/Lon)",
            "  Shift+Pfeil Feinrotation",
            "  + −         Zoom rein/raus",
            "  0           Zoom-Reset",
            "  h           Home-Position",
            "  s           Subsolar-Position",
            "  f           Freeze toggle (Pfeile → Sonne)",
            "  Space       Auto-Rotation toggle",
            "  , .         Rotations-Speed −/+",
            "  m           Modus blocks/ascii/plain",
            "  c           Wolken-Layer ein/aus",
            "  r           Defaults zurück (Position bleibt)",
            "  ?           Hilfe ein/aus",
            "  q / Esc     Beenden",
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

fn shade_color(
    ray: &Ray,
    sun_dir: V3,
    earth_rot: f64,
    pix_x: u32,
    pix_y: u32,
    cam_distance: f64,
    clouds_visible: bool,
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
                return rgb_to_ansi(245, 200, 90);
            }
        }
        rgb_to_ansi(final_rgb.0, final_rgb.1, final_rgb.2)
    } else {
        let perp = ray_perp_to_origin(ray);
        let g = glow_intensity(perp);
        if g > 0.0 {
            return rgb_to_ansi((30.0 * g) as u8, (70.0 * g) as u8, (150.0 * g) as u8);
        }
        if star_at(pix_x, pix_y) {
            return rgb_to_ansi(220, 220, 220);
        }
        16
    }
}

fn shade_ascii(
    ray: &Ray,
    sun_dir: V3,
    earth_rot: f64,
    pix_x: u32,
    pix_y: u32,
    cam_distance: f64,
    color: bool,
) -> (char, u8) {
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
                return ('·', if color { rgb_to_ansi(245, 200, 90) } else { 15 });
            }
            return (' ', 16);
        }

        // Gamma-Stretch + Klassen-Albedo: ohne diese beiden Korrekturen sehen alle
        // Pixel der Tagseite gleich hell aus und Kontinent-Konturen verschwinden.
        let lambert = raw.max(0.0).powf(ASCII_GAMMA);
        let visual = (lambert * class_albedo(class)).clamp(0.0, 1.0);
        let idx = ((visual * (RAMP.len() as f64 - 0.01)) as usize).min(RAMP.len() - 1);
        let ch = RAMP[idx];
        let fg = if color {
            let day = palette_day(class);
            // Farbe nutzt den reinen Lambert (ohne Albedo), damit Tag/Nacht-Verlauf
            // erhalten bleibt — die Klassen-Differenzierung kommt über die Klassenfarbe.
            let r = (day.0 as f64 * (0.3 + 0.7 * lambert)) as u8;
            let g = (day.1 as f64 * (0.3 + 0.7 * lambert)) as u8;
            let b = (day.2 as f64 * (0.3 + 0.7 * lambert)) as u8;
            rgb_to_ansi(r, g, b)
        } else {
            15
        };
        (ch, fg)
    } else {
        let perp = ray_perp_to_origin(ray);
        let gl = glow_intensity(perp);
        if gl > 0.5 {
            return ('.', if color { rgb_to_ansi(30, 70, 150) } else { 15 });
        }
        if star_at(pix_x, pix_y) {
            return ('*', if color { 250 } else { 15 });
        }
        (' ', 16)
    }
}

fn mix_rgb_u8(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| -> u8 {
        (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn moon_phase_char(illum: f64) -> char {
    // 0 = Neumond, 1 = Vollmond. Ohne Phasenrichtung — wir nehmen vereinfacht
    // einen Block, dessen Helligkeit der Phase entspricht.
    if illum < 0.05 {
        '·'
    } else if illum < 0.25 {
        '◗'
    } else if illum < 0.5 {
        '◐'
    } else if illum < 0.85 {
        '◑'
    } else {
        '●'
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ZOOM_DEFAULT;
    use chrono::TimeZone;

    fn now_fixed() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).single().unwrap()
    }

    #[test]
    fn new_sets_camera_to_home() {
        let app = AppState::new((48.21, 16.37), RenderMode::Blocks);
        assert!((app.camera.lat_deg - 48.21).abs() < 1e-9);
        assert!((app.camera.lon_deg - 16.37).abs() < 1e-9);
        assert_eq!(app.camera.distance, ZOOM_DEFAULT);
        assert_eq!(app.mode, RenderMode::Blocks);
        assert!(matches!(app.auto_rotation, AutoRotation::On { speed_idx: 0 }));
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
