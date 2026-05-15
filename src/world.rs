//! Erddaten-Sampling.
//!
//! V1 nutzt prozedurales Value-Noise — echte NASA-Maps (Klassen, Stadtlichter, Wolken)
//! sind als Drop-in-Replacement vorgesehen und ändern die öffentliche API nicht.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    DeepSea,
    Sea,
    Flatland,
    Upland,
    Mountain,
    Ice,
}

/// Klasse an gegebenem (Lat, Lon) in **Radian**.
pub fn sample_class(lat: f64, lon: f64) -> Class {
    if lat.abs() > 1.22 {
        // |lat| > ~70°
        return Class::Ice;
    }
    let n = continent_noise(lat, lon);
    match n {
        n if n < 0.42 => Class::DeepSea,
        n if n < 0.50 => Class::Sea,
        n if n < 0.58 => Class::Flatland,
        n if n < 0.70 => Class::Upland,
        _ => Class::Mountain,
    }
}

/// Stadtlicht-Stärke 0–255 an gegebenem (Lat, Lon).
/// Liefert 0 für Ozean / Eis und nicht-besiedelte Land-Pixel.
pub fn sample_lights(lat: f64, lon: f64) -> u8 {
    let cls = sample_class(lat, lon);
    if matches!(cls, Class::DeepSea | Class::Sea | Class::Ice) {
        return 0;
    }
    // Sparse "Stadt"-Cluster über höherfrequentes Noise mit Threshold.
    let city = fbm(lon * 18.0, lat * 18.0, 3);
    if city > 0.55 {
        let strength = ((city - 0.55) / 0.45).min(1.0);
        (strength * 255.0) as u8
    } else {
        0
    }
}

/// Wolken-Alpha 0–255.
pub fn sample_clouds(lat: f64, lon: f64) -> u8 {
    let cn = fbm(lon * 5.0 + 1.7, lat * 5.0, 4);
    if cn > 0.55 {
        let t = ((cn - 0.55) / 0.25).min(1.0);
        (t * 255.0 * 0.75) as u8
    } else {
        0
    }
}

// ---- Internes Noise -------------------------------------------------------

fn continent_noise(lat: f64, lon: f64) -> f64 {
    let x = (lon + PI) * 1.4;
    let y = (lat + PI / 2.0) * 1.4;
    fbm(x, y, 4)
}

fn hash2(x: f64, y: f64) -> f64 {
    let h = (x * 12.9898 + y * 78.233).sin() * 43758.5453;
    h - h.floor()
}

fn value_noise(x: f64, y: f64) -> f64 {
    let ix = x.floor();
    let iy = y.floor();
    let fx = x - ix;
    let fy = y - iy;
    let a = hash2(ix, iy);
    let b = hash2(ix + 1.0, iy);
    let c = hash2(ix, iy + 1.0);
    let d = hash2(ix + 1.0, iy + 1.0);
    let u = fx * fx * (3.0 - 2.0 * fx);
    let v = fy * fy * (3.0 - 2.0 * fy);
    a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + d * u * v
}

fn fbm(x: f64, y: f64, octaves: u32) -> f64 {
    let mut n = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut total = 0.0;
    for _ in 0..octaves {
        n += value_noise(x * freq, y * freq) * amp;
        total += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    n / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poles_are_ice() {
        // North pole, south pole
        assert_eq!(sample_class((85.0_f64).to_radians(), 0.0), Class::Ice);
        assert_eq!(sample_class((-85.0_f64).to_radians(), 0.0), Class::Ice);
    }

    #[test]
    fn sample_class_is_deterministic() {
        let a = sample_class(0.3, 1.2);
        let b = sample_class(0.3, 1.2);
        assert_eq!(a, b);
    }

    #[test]
    fn sample_lights_zero_on_sea_or_ice() {
        // High latitudes always ice → no lights.
        for lon_deg in (-180..180).step_by(30) {
            assert_eq!(sample_lights((80.0_f64).to_radians(), (lon_deg as f64).to_radians()), 0);
            assert_eq!(sample_lights((-80.0_f64).to_radians(), (lon_deg as f64).to_radians()), 0);
        }
    }

    #[test]
    fn class_distribution_covers_multiple_classes() {
        use std::collections::HashSet;
        let mut seen: HashSet<Class> = HashSet::new();
        // Equatorial belt
        for lat_deg in -60..=60 {
            for lon_deg in -180..180 {
                seen.insert(sample_class(
                    (lat_deg as f64).to_radians(),
                    (lon_deg as f64).to_radians(),
                ));
            }
        }
        // Mindestens DeepSea, Sea, Flatland, Upland, Mountain müssen vorkommen
        assert!(seen.contains(&Class::DeepSea), "DeepSea fehlt");
        assert!(seen.contains(&Class::Flatland), "Flatland fehlt");
        assert!(seen.len() >= 4, "Zu wenig Klassen: {:?}", seen);
    }

    #[test]
    fn clouds_return_some_coverage_and_some_clear_sky() {
        let mut covered = 0;
        let mut clear = 0;
        for lat_deg in (-80..80).step_by(5) {
            for lon_deg in (-180..180).step_by(5) {
                let c = sample_clouds(
                    (lat_deg as f64).to_radians(),
                    (lon_deg as f64).to_radians(),
                );
                if c > 0 {
                    covered += 1;
                } else {
                    clear += 1;
                }
            }
        }
        assert!(covered > 100 && clear > 100, "covered={}, clear={}", covered, clear);
    }

    #[test]
    fn lights_have_some_distribution() {
        let mut nonzero = 0;
        for lat_deg in -50..=50 {
            for lon_deg in (-180..180).step_by(2) {
                let l = sample_lights(
                    (lat_deg as f64).to_radians(),
                    (lon_deg as f64).to_radians(),
                );
                if l > 0 {
                    nonzero += 1;
                }
            }
        }
        // Erwartet: einige Stadt-Cluster, aber bei weitem nicht alle Land-Pixel.
        assert!(nonzero > 50, "Kaum Stadtlichter gefunden: {}", nonzero);
        let total = 101 * 180;
        assert!(nonzero < total / 4, "Zu viele Stadtlichter: {}/{}", nonzero, total);
    }
}
