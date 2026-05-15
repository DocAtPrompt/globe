//! Subsolar-Punkt aus Datum/Uhrzeit (vereinfachte NOAA-Formel, ≈0.01° genau).

use chrono::{DateTime, Datelike, Timelike, Utc};

use crate::vec3::{V3, from_lat_lon};

/// Julianisches Datum für UTC-Zeitpunkt (Meeus Algorithmus).
pub fn julian_day(now: DateTime<Utc>) -> f64 {
    let mut y = now.year() as i32;
    let mut m = now.month() as i32;
    let d = now.day() as f64;
    let day_frac = (now.hour() as f64
        + now.minute() as f64 / 60.0
        + now.second() as f64 / 3600.0)
        / 24.0;

    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = y.div_euclid(100);
    let b = 2 - a + a.div_euclid(4);

    (365.25 * (y + 4716) as f64).floor()
        + (30.6001 * (m + 1) as f64).floor()
        + d
        + day_frac
        + b as f64
        - 1524.5
}

/// Subsolar-Punkt in Grad: (lat, lon). Lat in [-23.45°, +23.45°], Lon in [-180°, 180°].
pub fn subsolar_point(now: DateTime<Utc>) -> (f64, f64) {
    let jd = julian_day(now);
    let n = jd - 2451545.0;

    let l_deg = (280.460 + 0.9856474 * n).rem_euclid(360.0);
    let g_rad = ((357.528 + 0.9856003 * n).rem_euclid(360.0)).to_radians();
    let lambda_rad =
        (l_deg + 1.915 * g_rad.sin() + 0.020 * (2.0 * g_rad).sin()).to_radians();
    let epsilon_rad = (23.439 - 0.0000004 * n).to_radians();

    let decl_deg = (epsilon_rad.sin() * lambda_rad.sin()).asin().to_degrees();

    let alpha_deg = (epsilon_rad.cos() * lambda_rad.sin())
        .atan2(lambda_rad.cos())
        .to_degrees()
        .rem_euclid(360.0);

    let gmst_hours = (18.697374558 + 24.06570982441908 * n).rem_euclid(24.0);
    let ha_green = (gmst_hours * 15.0 - alpha_deg).rem_euclid(360.0);

    let mut sub_lon = -ha_green;
    if sub_lon > 180.0 {
        sub_lon -= 360.0;
    }
    if sub_lon < -180.0 {
        sub_lon += 360.0;
    }

    (decl_deg, sub_lon)
}

/// Sonnen-Einheitsvektor im Welt-Koordinaten-System (gleiche Konvention wie [`vec3::from_lat_lon`]).
pub fn sun_direction(now: DateTime<Utc>) -> V3 {
    let (lat_deg, lon_deg) = subsolar_point(now);
    from_lat_lon(lat_deg.to_radians(), lon_deg.to_radians())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn julian_day_j2000_epoch() {
        let j2000 = dt(2000, 1, 1, 12, 0, 0);
        let jd = julian_day(j2000);
        assert!(close(jd, 2451545.0, 1e-6), "got {}", jd);
    }

    #[test]
    fn subsolar_lat_at_summer_solstice_2026() {
        let solstice = dt(2026, 6, 21, 8, 24, 0);
        let (lat, _lon) = subsolar_point(solstice);
        assert!(
            close(lat, 23.44, 0.2),
            "summer solstice lat expected ≈23.44°, got {:.3}",
            lat
        );
    }

    #[test]
    fn subsolar_lat_at_winter_solstice_2026() {
        let solstice = dt(2026, 12, 21, 15, 50, 0);
        let (lat, _lon) = subsolar_point(solstice);
        assert!(
            close(lat, -23.44, 0.2),
            "winter solstice lat expected ≈-23.44°, got {:.3}",
            lat
        );
    }

    #[test]
    fn subsolar_lat_at_spring_equinox_2026() {
        let equinox = dt(2026, 3, 20, 14, 46, 0);
        let (lat, _lon) = subsolar_point(equinox);
        assert!(
            close(lat, 0.0, 0.6),
            "spring equinox lat expected ≈0°, got {:.3}",
            lat
        );
    }

    #[test]
    fn subsolar_lat_at_autumn_equinox_2026() {
        let equinox = dt(2026, 9, 23, 0, 6, 0);
        let (lat, _lon) = subsolar_point(equinox);
        assert!(
            close(lat, 0.0, 0.6),
            "autumn equinox lat expected ≈0°, got {:.3}",
            lat
        );
    }

    #[test]
    fn subsolar_lon_at_noon_utc_is_near_zero() {
        // Equation of time at spring equinox ≈ -7 min → sub_lon ≈ -1.75°
        let noon = dt(2026, 3, 20, 12, 0, 0);
        let (_lat, lon) = subsolar_point(noon);
        assert!(
            lon.abs() < 5.0,
            "noon-UTC sub_lon expected within ±5° of 0°, got {:.2}",
            lon
        );
    }

    #[test]
    fn subsolar_lon_at_midnight_utc_is_near_180() {
        let midnight = dt(2026, 6, 21, 0, 0, 0);
        let (_lat, lon) = subsolar_point(midnight);
        assert!(
            lon.abs() > 170.0,
            "midnight-UTC sub_lon expected within ±10° of ±180°, got {:.2}",
            lon
        );
    }

    #[test]
    fn sun_direction_is_unit_vector() {
        let now = dt(2026, 5, 14, 12, 0, 0);
        let d = sun_direction(now);
        let len_sq = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
        assert!(close(len_sq, 1.0, 1e-10));
    }

    proptest::proptest! {
        #[test]
        fn subsolar_lat_within_obliquity_bounds(
            days in -3650i64..3650i64,
            seconds in 0u32..86_400u32,
        ) {
            let base = dt(2026, 1, 1, 0, 0, 0);
            let when = base + chrono::Duration::days(days) + chrono::Duration::seconds(seconds as i64);
            let (lat, lon) = subsolar_point(when);
            proptest::prop_assert!(lat.abs() <= 23.5,  "lat {} out of range",  lat);
            proptest::prop_assert!(lon >= -180.0 && lon <= 180.0, "lon {} out of range", lon);
        }
    }
}
