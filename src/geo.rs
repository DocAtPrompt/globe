//! Home-Position-Bestimmung: CLI-Override oder Timezone-Schätzung.

use chrono::{Local, Offset};

use crate::constants::{
    HOME_DEFAULT_LAT_NORTH, HOME_FALLBACK_LAT, HOME_FALLBACK_LON,
};

/// `--home`/`-h` CLI-Argument parsen. Erwartet `"LAT,LON"` in Grad.
/// Toleriert Whitespace und gibt aussagekräftige Fehler zurück.
pub fn parse_home_arg(s: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(format!(
            "erwartet 'LAT,LON' (zwei kommagetrennte Zahlen), bekommen: {:?}",
            s
        ));
    }
    let lat: f64 = parts[0]
        .parse()
        .map_err(|_| format!("Lat konnte nicht als Zahl gelesen werden: {:?}", parts[0]))?;
    let lon: f64 = parts[1]
        .parse()
        .map_err(|_| format!("Lon konnte nicht als Zahl gelesen werden: {:?}", parts[1]))?;
    if !lat.is_finite() || !lon.is_finite() {
        return Err("Lat/Lon dürfen nicht NaN oder unendlich sein".into());
    }
    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!("Lat außerhalb [-90, 90]: {}", lat));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!("Lon außerhalb [-180, 180]: {}", lon));
    }
    Ok((lat, lon))
}

/// Pure Funktion: aus UTC-Offset (Stunden) eine grobe Heimat-Position schätzen.
/// Lat-Default Nordhalbkugel; Lon ≈ 15° pro UTC-Offset-Stunde.
pub fn estimate_from_offset_hours(offset_hours: f64) -> (f64, f64) {
    let mut lon = offset_hours * 15.0;
    if lon > 180.0 {
        lon -= 360.0;
    }
    if lon < -180.0 {
        lon += 360.0;
    }
    (HOME_DEFAULT_LAT_NORTH, lon)
}

/// Aus der System-Timezone die Heimat-Position schätzen.
/// Bei unbekannter Timezone Fallback auf (0, 0).
pub fn estimate_from_timezone() -> (f64, f64) {
    // chrono::Local hat den Offset des laufenden Systems.
    let offset_seconds = Local::now().offset().fix().local_minus_utc();
    estimate_from_offset_hours(offset_seconds as f64 / 3600.0)
}

/// Komplette Home-Auflösung: CLI-Argument hat Priorität, sonst Timezone-Schätzung,
/// sonst harter Fallback.
pub fn resolve_home(cli_arg: Option<&str>) -> Result<(f64, f64), String> {
    if let Some(arg) = cli_arg {
        return parse_home_arg(arg);
    }
    // estimate_from_timezone gibt immer ein Tupel zurück – keine Panik möglich.
    let est = estimate_from_timezone();
    if est.0.is_finite() && est.1.is_finite() {
        Ok(est)
    } else {
        Ok((HOME_FALLBACK_LAT, HOME_FALLBACK_LON))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_home_arg_valid() {
        assert_eq!(parse_home_arg("48.21,16.37"), Ok((48.21, 16.37)));
        assert_eq!(parse_home_arg(" 48.21 , 16.37 "), Ok((48.21, 16.37)));
        assert_eq!(parse_home_arg("-33.86,151.21"), Ok((-33.86, 151.21)));
        assert_eq!(parse_home_arg("0,0"), Ok((0.0, 0.0)));
        assert_eq!(parse_home_arg("90,180"), Ok((90.0, 180.0)));
        assert_eq!(parse_home_arg("-90,-180"), Ok((-90.0, -180.0)));
    }

    #[test]
    fn parse_home_arg_too_few_or_many() {
        assert!(parse_home_arg("48.21").is_err());
        assert!(parse_home_arg("48.21,16.37,99").is_err());
        assert!(parse_home_arg("").is_err());
    }

    #[test]
    fn parse_home_arg_non_numeric() {
        assert!(parse_home_arg("foo,bar").is_err());
        assert!(parse_home_arg("48.21,xyz").is_err());
    }

    #[test]
    fn parse_home_arg_out_of_range() {
        assert!(parse_home_arg("91,0").is_err());
        assert!(parse_home_arg("-91,0").is_err());
        assert!(parse_home_arg("0,181").is_err());
        assert!(parse_home_arg("0,-181").is_err());
    }

    #[test]
    fn parse_home_arg_rejects_nan() {
        assert!(parse_home_arg("NaN,0").is_err());
        assert!(parse_home_arg("inf,0").is_err());
    }

    #[test]
    fn estimate_from_offset_known_zones() {
        // UTC
        assert_eq!(estimate_from_offset_hours(0.0).1, 0.0);
        // Europe/Vienna (CET = +1)
        assert!((estimate_from_offset_hours(1.0).1 - 15.0).abs() < 1e-9);
        // America/Los_Angeles (PST = -8)
        assert!((estimate_from_offset_hours(-8.0).1 + 120.0).abs() < 1e-9);
        // Pacific/Auckland (NZST = +12)
        assert!((estimate_from_offset_hours(12.0).1 - 180.0).abs() < 1e-9);
        // Default lat
        assert_eq!(estimate_from_offset_hours(0.0).0, HOME_DEFAULT_LAT_NORTH);
    }

    #[test]
    fn estimate_from_offset_wraps_above_180() {
        // Theoretisch wäre +13h → 195°, soll zu -165° werden
        let (_lat, lon) = estimate_from_offset_hours(13.0);
        assert!((lon - (-165.0)).abs() < 1e-9, "got {}", lon);
        let (_lat, lon) = estimate_from_offset_hours(-13.0);
        assert!((lon - 165.0).abs() < 1e-9, "got {}", lon);
    }

    #[test]
    fn estimate_from_timezone_returns_finite() {
        let (lat, lon) = estimate_from_timezone();
        assert!(lat.is_finite() && lon.is_finite());
        assert!((-90.0..=90.0).contains(&lat));
        assert!((-180.0..=180.0).contains(&lon));
    }

    #[test]
    fn resolve_home_cli_overrides_timezone() {
        let h = resolve_home(Some("48.21,16.37")).unwrap();
        assert_eq!(h, (48.21, 16.37));
    }

    #[test]
    fn resolve_home_falls_back_to_timezone() {
        let h = resolve_home(None).unwrap();
        assert!(h.0.is_finite() && h.1.is_finite());
    }

    #[test]
    fn resolve_home_invalid_cli_returns_err() {
        assert!(resolve_home(Some("nope")).is_err());
    }
}
