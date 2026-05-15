//! Raycasting, Beleuchtung, Klassen-Shading und Frame-Render.

use crate::camera::Camera;
use crate::constants::{CLOUD_RADIUS, FOV_HALF, GLOW_OUTER, STAR_DENSITY, SUN_MOON_VISIBLE_DIST};
use crate::moon;
use crate::sun;
use crate::vec3::{V3, from_lat_lon, rotate_y, to_lat_lon};

// `rotate_y` wird auch für sun/moon-Marker (Erd-fixed → Welt-Frame) gebraucht.
use crate::world::{self, Class};

use chrono::{DateTime, Utc};

// ----- Ray + Sphere ---------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: V3,
    pub dir: V3,
}

#[derive(Clone, Copy, Debug)]
pub struct SphereHit {
    pub t: f64,
    pub point: V3,
    /// Perpendicular Distance vom Strahl zum Sphere-Mittelpunkt
    pub perp: f64,
}

/// Kleinster positiver `t` der Schnittpunkt mit Sphere (Ursprung, Radius).
pub fn ray_sphere(ray: &Ray, radius: f64) -> Option<SphereHit> {
    let cam = ray.origin;
    let dir = ray.dir;
    let t_close = -cam.dot(dir);
    let perp2 = cam.dot(cam) - t_close * t_close;
    let r2 = radius * radius;
    if perp2 >= r2 {
        return None;
    }
    let t = t_close - (r2 - perp2).sqrt();
    if t <= 0.0 {
        return None;
    }
    let point = cam.add(dir.mul(t));
    Some(SphereHit {
        t,
        point,
        perp: perp2.max(0.0).sqrt(),
    })
}

/// Perpendicular Distance vom Strahl zum Origin (auch bei No-Hit).
pub fn ray_perp_to_origin(ray: &Ray) -> f64 {
    let cam = ray.origin;
    let dir = ray.dir;
    let t_close = -cam.dot(dir);
    let perp2 = (cam.dot(cam) - t_close * t_close).max(0.0);
    perp2.sqrt()
}

// ----- Kamera + Ray-Construction --------------------------------------------

/// Erzeugt Strahl für ein Sub-Pixel.
/// `sx` in [0, width), `sy` in [0, sub_height). Sample wird zentriert (+0.5).
pub fn ray_for_sub_pixel(
    sx: f64,
    sy: f64,
    width: f64,
    sub_height: f64,
    cam: &Camera,
) -> Ray {
    ray_at_screen(sx + 0.5, sy + 0.5, width, sub_height, cam)
}

/// Variante ohne +0.5-Center-Offset: `sx`/`sy` werden direkt als Position
/// interpretiert. Praktisch wenn die Caller-Mathematik nicht-integer-Sample-
/// Positionen erzeugt (z. B. Halbblock-Sub-Pixel bei nicht-1:2-Cell-Aspect).
pub fn ray_at_screen(
    sx: f64,
    sy: f64,
    width: f64,
    sub_height: f64,
    cam: &Camera,
) -> Ray {
    let aspect = width / sub_height;
    let tan_fov = FOV_HALF.tan();
    let u = (2.0 * sx / width - 1.0) * aspect * tan_fov;
    let v = (1.0 - 2.0 * sy / sub_height) * tan_fov;

    let cam_dir = from_lat_lon(cam.lat_deg.to_radians(), cam.lon_deg.to_radians());
    let origin = cam_dir.mul(cam.distance);
    let forward = cam_dir.mul(-1.0);
    let world_up = V3(0.0, 1.0, 0.0);
    // Geographisches "rechts" = nach Osten: right = up × forward (nicht forward × up).
    // Sonst zeigt das Bild horizontal gespiegelt — Indien links statt rechts von Afrika.
    let right = world_up.cross(forward).norm();
    let up = forward.cross(right).norm();

    let dir = forward.add(right.mul(u)).add(up.mul(v)).norm();
    Ray { origin, dir }
}

// ----- Hit → Geo ------------------------------------------------------------

/// Hit-Punkt im Welt-Koord → (Lat, Lon) in Radian, nach Rückrotation um die Erddrehung.
pub fn hit_to_geo(p: V3, earth_rotation_rad: f64) -> (f64, f64) {
    let local = rotate_y(p, -earth_rotation_rad);
    to_lat_lon(local)
}

// ----- Beleuchtung ----------------------------------------------------------

/// `light` ∈ [0, 1] mit weichem Terminator.
pub fn lighting(normal_world: V3, sun_dir: V3) -> f64 {
    let raw = normal_world.dot(sun_dir);
    smoothstep(-0.1, 0.1, raw)
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ----- ANSI 256 Konvertierung -----------------------------------------------

/// RGB (0–255) → ANSI-256-Index (6×6×6-Würfel + Grau-Rampe).
pub fn rgb_to_ansi(r: u8, g: u8, b: u8) -> u8 {
    // Greyscale-Shortcut
    if r == g && g == b {
        if r < 8 {
            16
        } else if r > 248 {
            231
        } else {
            232 + ((r as u16 - 8) * 24 / 247) as u8
        }
    } else {
        let to6 = |v: u8| -> u8 {
            if v < 48 {
                0
            } else if v < 115 {
                1
            } else {
                (((v as u16).saturating_sub(35)) / 40).min(5) as u8
            }
        };
        16 + 36 * to6(r) + 6 * to6(g) + to6(b)
    }
}

// ----- Klassen-Paletten ------------------------------------------------------

pub fn palette_day(c: Class) -> (u8, u8, u8) {
    match c {
        Class::DeepSea => (10, 20, 70),
        Class::Sea => (30, 80, 140),
        Class::Flatland => (50, 120, 50),
        Class::Upland => (120, 100, 50),
        Class::Mountain => (140, 140, 140),
        Class::Ice => (220, 220, 230),
    }
}

pub fn palette_night(c: Class) -> (u8, u8, u8) {
    match c {
        Class::DeepSea => (3, 6, 20),
        Class::Sea => (8, 20, 40),
        Class::Flatland => (15, 40, 20),
        Class::Upland => (40, 35, 20),
        Class::Mountain => (50, 50, 50),
        Class::Ice => (100, 100, 110),
    }
}

fn mix_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| -> u8 {
        (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

pub fn class_color(c: Class, light: f64) -> (u8, u8, u8) {
    mix_rgb(palette_night(c), palette_day(c), light)
}

// ----- Stadtlichter ---------------------------------------------------------

pub const CITY_LIGHT_RGB: (u8, u8, u8) = (245, 200, 90);

/// Schwellwert für sichtbare Stadtlicht-Stärke, abhängig vom Zoom.
/// Bei `distance = ZOOM_DEFAULT` nur hellste Lichter, beim Reinzoomen mehr.
pub fn lights_visible(strength: u8, distance: f64) -> bool {
    use crate::constants::{ZOOM_DEFAULT, ZOOM_MIN};
    if strength == 0 {
        return false;
    }
    // Bei ZOOM_DEFAULT: threshold = ~200; bei ZOOM_MIN: threshold = ~30.
    let t = ((distance - ZOOM_MIN) / (ZOOM_DEFAULT - ZOOM_MIN)).clamp(0.0, 2.0);
    let threshold = 30.0 + t * 170.0;
    strength as f64 >= threshold
}

// ----- Atmosphären-Glow + Sterne ------------------------------------------

pub fn glow_intensity(perp: f64) -> f64 {
    if perp <= 1.0 || perp >= GLOW_OUTER {
        return 0.0;
    }
    let t = (GLOW_OUTER - perp) / (GLOW_OUTER - 1.0);
    t * t
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Star {
    Bright,
    Medium,
    Dim,
}

pub fn star_at(x: u32, y: u32) -> Option<Star> {
    let h = hash_u32(x, y);
    if h < STAR_DENSITY * 0.4 {
        Some(Star::Bright)
    } else if h < STAR_DENSITY * 1.4 {
        Some(Star::Medium)
    } else if h < STAR_DENSITY * 3.5 {
        Some(Star::Dim)
    } else {
        None
    }
}

/// Hash mit zwei Misch-Schritten — der alte XOR-Ansatz erzeugte sichtbare
/// Diagonalen, weil x und y in benachbarten Zellen quasi-linear korrelieren.
fn hash_u32(x: u32, y: u32) -> f64 {
    let mut v = x.wrapping_mul(2_654_435_761).wrapping_add(y.wrapping_mul(1_597_334_677));
    v ^= v >> 16;
    v = v.wrapping_mul(0x85eb_ca6b);
    v ^= v >> 13;
    v = v.wrapping_mul(0xc2b2_ae35);
    v ^= v >> 16;
    (v as f64) / (u32::MAX as f64)
}

// ----- Shade-Funktion (top-level): Sub-Pixel → ANSI-Color -------------------

pub struct ShadeInput<'a> {
    pub ray: Ray,
    pub sun_dir: V3,
    pub earth_rotation_rad: f64,
    pub pixel_x: u32,
    pub pixel_y: u32,
    pub camera_distance: f64,
    pub _phantom: std::marker::PhantomData<&'a ()>,
}

pub fn shade_sub_pixel(input: &ShadeInput) -> u8 {
    // Erd-Sphere
    let earth = ray_sphere(&input.ray, 1.0);

    if let Some(hit) = earth {
        let (lat_rad, lon_rad) = hit_to_geo(hit.point, input.earth_rotation_rad);
        let class = world::sample_class(lat_rad, lon_rad);
        let light = lighting(hit.point, input.sun_dir);

        // Cloud-Sphere check (näher zur Kamera als Erd-Hit)
        let cloud_hit = ray_sphere(&input.ray, CLOUD_RADIUS);
        let cloud_color = if let Some(ch) = cloud_hit {
            if ch.t < hit.t {
                let (clat, clon) = hit_to_geo(ch.point, input.earth_rotation_rad * 1.2);
                let alpha = world::sample_clouds(clat, clon) as f64 / 255.0;
                if alpha > 0.0 {
                    let base = class_color(class, light);
                    let cloud_lit = ((230.0 * light) as u8).max(20);
                    let mixed = mix_rgb(base, (cloud_lit, cloud_lit, cloud_lit), alpha);
                    Some(mixed)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Stadtlichter (nur auf Nachtseite)
        if light < 0.05 {
            let strength = world::sample_lights(lat_rad, lon_rad);
            if lights_visible(strength, input.camera_distance) {
                return rgb_to_ansi(CITY_LIGHT_RGB.0, CITY_LIGHT_RGB.1, CITY_LIGHT_RGB.2);
            }
        }

        let (r, g, b) = cloud_color.unwrap_or_else(|| class_color(class, light));
        rgb_to_ansi(r, g, b)
    } else {
        // Atmosphären-Glow oder Hintergrund
        let perp = ray_perp_to_origin(&input.ray);
        let glow = glow_intensity(perp);
        if glow > 0.0 {
            let r = (30.0 * glow) as u8;
            let g = (70.0 * glow) as u8;
            let b = (150.0 * glow) as u8;
            return rgb_to_ansi(r, g, b);
        }
        if star_at(input.pixel_x, input.pixel_y).is_some() {
            return rgb_to_ansi(220, 220, 220);
        }
        16 // schwarz (ANSI)
    }
}

// ----- Sonne + Mond Projektion ----------------------------------------------

/// Zeigt Sonne/Mond als Punkte ab `SUN_MOON_VISIBLE_DIST`. Bei kleineren Distanzen
/// liegt die Sonne praktisch im Unendlichen und der Mond hinter/außerhalb des Bildes.
pub fn sun_moon_visible(distance: f64) -> bool {
    distance >= SUN_MOON_VISIBLE_DIST
}

/// Projeziere Welt-Punkt P auf Sub-Pixel-Koord. `None` wenn hinter Kamera oder außerhalb.
pub fn project(p: V3, cam: &Camera, width: f64, sub_height: f64) -> Option<(f64, f64)> {
    let cam_dir = from_lat_lon(cam.lat_deg.to_radians(), cam.lon_deg.to_radians());
    let origin = cam_dir.mul(cam.distance);
    let forward = cam_dir.mul(-1.0);
    let world_up = V3(0.0, 1.0, 0.0);
    // Selbe Right/Up-Konvention wie in `ray_for_sub_pixel`.
    let right = world_up.cross(forward).norm();
    let up = forward.cross(right).norm();

    let d = p.sub(origin);
    let z = d.dot(forward);
    if z <= 0.0 {
        return None;
    }
    let aspect = width / sub_height;
    let tan_fov = FOV_HALF.tan();
    let u = d.dot(right) / z / tan_fov;
    let v = d.dot(up) / z / tan_fov;
    if u.abs() > aspect || v.abs() > 1.0 {
        return None;
    }
    let sx = (u / aspect + 1.0) * 0.5 * width;
    let sy = (1.0 - v) * 0.5 * sub_height;
    Some((sx, sy))
}

/// Liefert `true`, wenn die Erdkugel (Sphere, Radius 1) die Sichtlinie zur Position
/// blockt — d. h. wenn der Strahl von der Kamera zum Target zwischen Kamera und Target
/// die Erde schneidet.
pub fn occluded_by_earth(target_world: V3, cam: &Camera) -> bool {
    let cam_dir = from_lat_lon(cam.lat_deg.to_radians(), cam.lon_deg.to_radians());
    let origin = cam_dir.mul(cam.distance);
    let to_target = target_world.sub(origin);
    let dist = to_target.len();
    if dist <= 0.0 {
        return false;
    }
    let ray = Ray {
        origin,
        dir: to_target.mul(1.0 / dist),
    };
    if let Some(hit) = ray_sphere(&ray, 1.0) {
        hit.t < dist
    } else {
        false
    }
}

/// Sonnen-Pixel-Position für aktuelle Zeit (oder None, wenn nicht sichtbar).
/// `earth_rotation_rad` rotiert den Erd-fixed-Sonnenvektor in den Welt-Frame,
/// damit die Sonne bei aktiver Erdrotation an ihrer wahren astronomischen Position
/// bleibt (und nicht mit der Erde mitwandert).
pub fn sun_marker(
    now: DateTime<Utc>,
    earth_rotation_rad: f64,
    cam: &Camera,
    width: f64,
    sub_height: f64,
) -> Option<(f64, f64)> {
    if !sun_moon_visible(cam.distance) {
        return None;
    }
    let sd_earth = sun::sun_direction(now);
    let sd_world = rotate_y(sd_earth, earth_rotation_rad);
    let target = sd_world.mul(10_000.0);
    if occluded_by_earth(target, cam) {
        return None;
    }
    project(target, cam, width, sub_height)
}

/// Mond-Pixel-Position + Illumination (oder None, wenn nicht sichtbar).
pub fn moon_marker(
    now: DateTime<Utc>,
    earth_rotation_rad: f64,
    cam: &Camera,
    width: f64,
    sub_height: f64,
) -> Option<((f64, f64), f64)> {
    if !sun_moon_visible(cam.distance) {
        return None;
    }
    let p_earth = moon::position_world(now);
    let p_world = rotate_y(p_earth, earth_rotation_rad);
    if occluded_by_earth(p_world, cam) {
        return None;
    }
    let illum = moon::illumination(now);
    project(p_world, cam, width, sub_height).map(|xy| (xy, illum))
}

// ----- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::from_lat_lon;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    fn ray_hitting_sphere() -> Ray {
        Ray {
            origin: V3(0.0, 0.0, 3.0),
            dir: V3(0.0, 0.0, -1.0),
        }
    }

    #[test]
    fn ray_sphere_hits_center() {
        let h = ray_sphere(&ray_hitting_sphere(), 1.0).unwrap();
        assert!(close(h.t, 2.0, 1e-12));
        assert!(close(h.point.0, 0.0, 1e-12));
        assert!(close(h.point.1, 0.0, 1e-12));
        assert!(close(h.point.2, 1.0, 1e-12));
        assert!(close(h.perp, 0.0, 1e-9));
    }

    #[test]
    fn ray_sphere_misses_when_far() {
        let r = Ray {
            origin: V3(0.0, 0.0, 3.0),
            dir: V3(1.0, 0.0, 0.0),
        };
        assert!(ray_sphere(&r, 1.0).is_none());
    }

    #[test]
    fn ray_sphere_misses_when_backwards() {
        let r = Ray {
            origin: V3(0.0, 0.0, 3.0),
            dir: V3(0.0, 0.0, 1.0),
        };
        assert!(ray_sphere(&r, 1.0).is_none());
    }

    #[test]
    fn ray_perp_to_origin_matches_hit() {
        let r = ray_hitting_sphere();
        assert!(close(ray_perp_to_origin(&r), 0.0, 1e-9));
    }

    #[test]
    fn ray_for_center_pixel_points_at_earth() {
        // Kamera bei (3,0,0), schaut auf Origin. Center-Ray sollte fast genau -x sein.
        let cam = Camera::new(0.0, 0.0);
        let ray = ray_for_sub_pixel(40.0, 20.0, 80.0, 40.0, &cam);
        assert!(ray.dir.0 < -0.95, "dir.x={} should be ~-1", ray.dir.0);
        assert!(ray.dir.1.abs() < 0.05, "dir.y={}", ray.dir.1);
        assert!(ray.dir.2.abs() < 0.05, "dir.z={}", ray.dir.2);
    }

    #[test]
    fn center_ray_hits_sphere_front() {
        let cam = Camera::new(0.0, 0.0);
        let ray = ray_for_sub_pixel(40.0, 20.0, 80.0, 40.0, &cam);
        let hit = ray_sphere(&ray, 1.0).unwrap();
        assert!((hit.t - 2.0).abs() < 0.05, "t={}, expected ≈2", hit.t);
    }

    #[test]
    fn hit_to_geo_no_rotation() {
        let p = from_lat_lon(0.0, 0.0); // (1, 0, 0)
        let (lat, lon) = hit_to_geo(p, 0.0);
        assert!(close(lat, 0.0, 1e-9));
        assert!(close(lon, 0.0, 1e-9));
    }

    #[test]
    fn hit_to_geo_reverses_earth_rotation() {
        use crate::vec3::rotate_y;
        let earth_lat = 0.0;
        let earth_lon = 0.3;
        let rot = 0.5;
        // Simuliere: Erd-Punkt nach Rotation um `rot` in Welt-Koord
        let p_world = rotate_y(from_lat_lon(earth_lat, earth_lon), rot);
        let (lat_back, lon_back) = hit_to_geo(p_world, rot);
        assert!(close(lat_back, earth_lat, 1e-9));
        assert!(close(lon_back, earth_lon, 1e-9));
    }

    #[test]
    fn lighting_clamps_and_smoothes() {
        let normal = V3(1.0, 0.0, 0.0);
        let sun_same = V3(1.0, 0.0, 0.0);
        let sun_opposite = V3(-1.0, 0.0, 0.0);
        let sun_tangent = V3(0.0, 1.0, 0.0);
        assert!(lighting(normal, sun_same) > 0.99);
        assert!(lighting(normal, sun_opposite) < 0.01);
        // Tangent: light value at smoothstep midpoint = 0.5
        let t = lighting(normal, sun_tangent);
        assert!((0.4..=0.6).contains(&t), "tangent light = {}", t);
    }

    #[test]
    fn rgb_to_ansi_known_anchors() {
        // Schwarz → 16
        assert_eq!(rgb_to_ansi(0, 0, 0), 16);
        // Weiß → 231
        assert_eq!(rgb_to_ansi(255, 255, 255), 231);
        // Mittelgrau → grayscale ramp
        let g = rgb_to_ansi(128, 128, 128);
        assert!((232..=255).contains(&g), "got {}", g);
    }

    #[test]
    fn class_color_day_brighter_than_night() {
        let c = Class::Flatland;
        let day = class_color(c, 1.0);
        let night = class_color(c, 0.0);
        let brightness = |x: (u8, u8, u8)| x.0 as u32 + x.1 as u32 + x.2 as u32;
        assert!(brightness(day) > brightness(night));
    }

    #[test]
    fn glow_intensity_falls_off_smoothly() {
        assert_eq!(glow_intensity(1.0), 0.0);
        assert_eq!(glow_intensity(GLOW_OUTER), 0.0);
        let middle = glow_intensity((1.0 + GLOW_OUTER) / 2.0);
        assert!(middle > 0.0 && middle < 1.0);
    }

    #[test]
    fn lights_visible_threshold_zoom_dependent() {
        use crate::constants::{ZOOM_DEFAULT, ZOOM_MIN};
        // Bei Default-Zoom: nur sehr helle Pixel
        assert!(lights_visible(250, ZOOM_DEFAULT));
        assert!(!lights_visible(100, ZOOM_DEFAULT));
        // Bei ZOOM_MIN: auch dunklere
        assert!(lights_visible(50, ZOOM_MIN));
    }

    #[test]
    fn sun_moon_visible_threshold() {
        use crate::constants::SUN_MOON_VISIBLE_DIST;
        assert!(!sun_moon_visible(SUN_MOON_VISIBLE_DIST - 0.1));
        assert!(sun_moon_visible(SUN_MOON_VISIBLE_DIST));
        assert!(sun_moon_visible(SUN_MOON_VISIBLE_DIST + 5.0));
    }

    #[test]
    fn occluded_by_earth_blocks_when_behind() {
        let cam = Camera::new(0.0, 0.0);
        // Target hinter der Erde (auf der gegenüberliegenden Seite)
        let behind = V3(-1000.0, 0.0, 0.0);
        assert!(occluded_by_earth(behind, &cam));
    }

    #[test]
    fn occluded_by_earth_not_when_above() {
        let cam = Camera::new(0.0, 0.0);
        // Target weit oben (Nord-Pol-Richtung) → nicht von Sphere blockiert
        let above = V3(0.0, 1000.0, 0.0);
        assert!(!occluded_by_earth(above, &cam));
    }

    #[test]
    fn project_center_of_view() {
        let cam = Camera::new(0.0, 0.0);
        let center_world = V3(0.0, 0.0, 0.0); // origin
        let p = project(center_world, &cam, 80.0, 40.0).unwrap();
        // Center of screen
        assert!(close(p.0, 40.0, 0.5));
        assert!(close(p.1, 20.0, 0.5));
    }
}
