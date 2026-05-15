//! Mondbahn + Phase (vereinfachte Meeus-Formel, ≈1° in Lat/Lon).

use chrono::{DateTime, Utc};

use crate::sun::{julian_day, sun_direction};
use crate::vec3::{from_lat_lon, V3};

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Anzeige-Distanz des Monds, in Welt-Einheiten (Erdradien). Echte ≈ 60.
/// Bei Original-Distanz steht der Mond praktisch immer außerhalb des FOV
/// (außer bei extremen Zoom-Out). Auf 6 komprimiert bleibt er bei normalem
/// Zoom als Begleiter neben der Erde sichtbar. Mondphase und Bahn-Geometrie
/// bleiben physikalisch korrekt; nur die Skalierung ist artistisch.
pub const MOON_DISPLAY_DISTANCE_EARTH_RADII: f64 = 6.0;

/// Subselenes Lat/Lon-Paar in Grad + Distance vom Erd-Zentrum in Erdradien.
/// Lat ≈ ±28.6° max (5° Bahn-Inklination + 23.4° Achsen-Neigung).
pub fn position(now: DateTime<Utc>) -> SubLunar {
    let jd = julian_day(now);
    let n = jd - 2451545.0;
    let t = n / 36525.0;

    // Mittlere ekliptische Elemente (Meeus-vereinfacht)
    let l_moon = (218.316 + 481267.8813 * t).rem_euclid(360.0);
    let m_moon = (134.963 + 477198.8676 * t).rem_euclid(360.0).to_radians();
    let f_moon = (93.272 + 483202.0175 * t).rem_euclid(360.0).to_radians();

    let lambda_deg = l_moon + 6.289 * m_moon.sin();
    let beta_deg = 5.128 * f_moon.sin();
    let dist_km = 385_001.0 - 20_905.0 * m_moon.cos();

    // Ekliptik → Äquatorial (Schiefe ε)
    let eps_rad = (23.439 - 0.0000004 * n).to_radians();
    let lambda_rad = lambda_deg.to_radians();
    let beta_rad = beta_deg.to_radians();

    let sin_decl =
        beta_rad.sin() * eps_rad.cos() + beta_rad.cos() * eps_rad.sin() * lambda_rad.sin();
    let decl_rad = sin_decl.clamp(-1.0, 1.0).asin();
    let alpha_rad =
        (lambda_rad.sin() * eps_rad.cos() - beta_rad.tan() * eps_rad.sin()).atan2(lambda_rad.cos());
    let alpha_deg = alpha_rad.to_degrees().rem_euclid(360.0);

    // GMST → Sublunar-Lon (gleiche Konvention wie sun::subsolar_point)
    let gmst_hours = (18.697374558 + 24.06570982441908 * n).rem_euclid(24.0);
    let ha_green = (gmst_hours * 15.0 - alpha_deg).rem_euclid(360.0);
    let mut sub_lon = -ha_green;
    if sub_lon > 180.0 {
        sub_lon -= 360.0;
    }
    if sub_lon < -180.0 {
        sub_lon += 360.0;
    }

    SubLunar {
        lat_deg: decl_rad.to_degrees(),
        lon_deg: sub_lon,
        distance_earth_radii: dist_km / EARTH_RADIUS_KM,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SubLunar {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub distance_earth_radii: f64,
}

/// Welt-Position des Monds in Erdradien (Ursprung = Erdmittelpunkt). Die echte
/// Bahn ist ~60 Erdradien entfernt; wir komprimieren auf
/// `MOON_DISPLAY_DISTANCE_EARTH_RADII`, damit der Mond im typischen Zoom-Bereich
/// neben der Erde sichtbar ist (Richtung und Phase bleiben astronomisch korrekt).
pub fn position_world(now: DateTime<Utc>) -> V3 {
    let s = position(now);
    let dir = from_lat_lon(s.lat_deg.to_radians(), s.lon_deg.to_radians());
    dir.mul(MOON_DISPLAY_DISTANCE_EARTH_RADII)
}

/// Beleuchtungs-Anteil 0…1 (0 = Neumond, 1 = Vollmond).
pub fn illumination(now: DateTime<Utc>) -> f64 {
    let sun = sun_direction(now);
    let m = position(now);
    let moon_dir = from_lat_lon(m.lat_deg.to_radians(), m.lon_deg.to_radians());
    // phase_angle = angle Erde–Mond–Sonne aus Erd-Sicht
    let cos_elong = sun.dot(moon_dir).clamp(-1.0, 1.0);
    // illumination = (1 - cos(180° - elong)) / 2 = (1 + cos(elong)) / 2
    // Wait: bei Vollmond ist Mond gegenüber Sonne → Elongation = 180° → cos = -1.
    // illum bei Vollmond = 1: → (1 - cos(elong)) / 2.
    (1.0 - cos_elong) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    #[test]
    fn distance_in_physical_range() {
        let s = position(dt(2026, 5, 15, 0, 0, 0));
        let km = s.distance_earth_radii * EARTH_RADIUS_KM;
        assert!(
            (355_000.0..=410_000.0).contains(&km),
            "Mond-Distance außerhalb plausibler Range: {} km",
            km
        );
    }

    #[test]
    fn declination_in_obliquity_plus_inclination_bound() {
        let s = position(dt(2026, 5, 15, 0, 0, 0));
        assert!(s.lat_deg.abs() < 30.0, "lat {} außerhalb ±30°", s.lat_deg);
    }

    #[test]
    fn full_moon_2026_03_03_high_illumination() {
        // Time and Date.com: Voller Mond 2026-03-03 11:38 UTC
        let full = dt(2026, 3, 3, 11, 38, 0);
        let i = illumination(full);
        assert!(i > 0.9, "Vollmond-Illum erwartet >0.9, got {:.3}", i);
    }

    #[test]
    fn new_moon_2026_02_17_low_illumination() {
        // Time and Date.com: Neumond 2026-02-17 12:01 UTC
        let new = dt(2026, 2, 17, 12, 1, 0);
        let i = illumination(new);
        assert!(i < 0.1, "Neumond-Illum erwartet <0.1, got {:.3}", i);
    }

    #[test]
    fn position_world_uses_compressed_distance() {
        // `position_world` skaliert artistisch auf MOON_DISPLAY_DISTANCE_EARTH_RADII,
        // damit der Mond bei normalem Zoom sichtbar bleibt.
        let now = dt(2026, 5, 15, 0, 0, 0);
        let p = position_world(now);
        let len = (p.0 * p.0 + p.1 * p.1 + p.2 * p.2).sqrt();
        assert!((len - MOON_DISPLAY_DISTANCE_EARTH_RADII).abs() < 1e-6);
    }

    proptest::proptest! {
        #[test]
        fn properties_over_decade(
            days in -3650i64..3650i64,
        ) {
            let base = dt(2026, 1, 1, 0, 0, 0);
            let when = base + chrono::Duration::days(days);
            let s = position(when);
            let i = illumination(when);
            proptest::prop_assert!(s.lat_deg.abs() < 30.0, "lat out of range: {}", s.lat_deg);
            proptest::prop_assert!(s.lon_deg >= -180.0 && s.lon_deg <= 180.0);
            let km = s.distance_earth_radii * EARTH_RADIUS_KM;
            proptest::prop_assert!((350_000.0..=415_000.0).contains(&km),
                "distance out of range: {} km", km);
            proptest::prop_assert!((0.0..=1.0).contains(&i), "illum out of [0,1]: {}", i);
        }
    }
}
