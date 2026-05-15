//! Erddaten-Sampling.
//!
//! Drei zur Compile-Zeit eingebettete Maps (Klassen, Stadtlichter, Wolken),
//! deflate-komprimiert. Sourcen: NASA Blue Marble + Black Marble + Cloud-Cover
//! via Three.js-Texture-Mirror (Public Domain). Konvertierung erfolgt durch
//! `tools/build_assets.py`.

use std::sync::OnceLock;

use miniz_oxide::inflate::decompress_to_vec_zlib;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    DeepSea,
    Sea,
    Flatland,
    Upland,
    Mountain,
    Ice,
}

/// Roh-Bytes der eingebetteten Maps. Format pro Datei:
/// MAGIC(4) "GLBE" + VERSION(u8) + WIDTH(u16 LE) + HEIGHT(u16 LE) + zlib-payload.
static CLASS_MAP_Z: &[u8] = include_bytes!("../assets/earth_classes.bin.z");
static LIGHTS_MAP_Z: &[u8] = include_bytes!("../assets/earth_lights.bin.z");
static CLOUDS_MAP_Z: &[u8] = include_bytes!("../assets/earth_clouds.bin.z");

const MAGIC: &[u8; 4] = b"GLBE";
const SUPPORTED_VERSION: u8 = 1;

struct Map {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Map {
    fn sample(&self, lat: f64, lon: f64) -> u8 {
        // lat in [-π/2, π/2] (Nordpol +), lon in [-π, π].
        // Map: y=0 ist Nordpol, x=0 ist lon = -π.
        let lat_norm = (std::f64::consts::FRAC_PI_2 - lat) / std::f64::consts::PI;
        let lon_norm = (lon + std::f64::consts::PI) / (2.0 * std::f64::consts::PI);
        let lat_norm = lat_norm.clamp(0.0, 1.0);
        let lon_norm = lon_norm.rem_euclid(1.0);
        let y = ((lat_norm * self.height as f64) as usize).min(self.height - 1);
        let x = ((lon_norm * self.width as f64) as usize) % self.width;
        self.data[y * self.width + x]
    }
}

fn load(asset: &[u8]) -> Map {
    assert!(asset.len() >= 9, "globe: asset header truncated");
    assert_eq!(&asset[..4], MAGIC, "globe: asset magic mismatch");
    assert_eq!(asset[4], SUPPORTED_VERSION, "globe: asset version mismatch");
    let width = u16::from_le_bytes([asset[5], asset[6]]) as usize;
    let height = u16::from_le_bytes([asset[7], asset[8]]) as usize;
    let data = decompress_to_vec_zlib(&asset[9..])
        .expect("globe: asset decompression failed (corrupt embedded data)");
    assert_eq!(
        data.len(),
        width * height,
        "globe: decompressed size mismatch"
    );
    Map { width, height, data }
}

fn class_map() -> &'static Map {
    static M: OnceLock<Map> = OnceLock::new();
    M.get_or_init(|| load(CLASS_MAP_Z))
}
fn lights_map() -> &'static Map {
    static M: OnceLock<Map> = OnceLock::new();
    M.get_or_init(|| load(LIGHTS_MAP_Z))
}
fn clouds_map() -> &'static Map {
    static M: OnceLock<Map> = OnceLock::new();
    M.get_or_init(|| load(CLOUDS_MAP_Z))
}

/// Klasse an gegebenem (Lat, Lon) in **Radian**.
pub fn sample_class(lat: f64, lon: f64) -> Class {
    match class_map().sample(lat, lon) {
        0 => Class::DeepSea,
        1 => Class::Sea,
        2 => Class::Flatland,
        3 => Class::Upland,
        4 => Class::Mountain,
        5 => Class::Ice,
        _ => Class::Sea,
    }
}

/// Stadtlicht-Stärke 0–255. Wasser- und Eis-Pixel werden hart auf 0 gesetzt
/// (die Black-Marble-Quelle hat dort manchmal Glow durch Atmosphären-Streuung).
pub fn sample_lights(lat: f64, lon: f64) -> u8 {
    let cls = sample_class(lat, lon);
    if matches!(cls, Class::DeepSea | Class::Sea | Class::Ice) {
        return 0;
    }
    lights_map().sample(lat, lon)
}

/// Wolken-Alpha 0–255.
pub fn sample_clouds(lat: f64, lon: f64) -> u8 {
    clouds_map().sample(lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rad(deg: f64) -> f64 { deg.to_radians() }

    #[test]
    fn sample_class_is_deterministic() {
        let a = sample_class(0.3, 1.2);
        let b = sample_class(0.3, 1.2);
        assert_eq!(a, b);
    }

    #[test]
    fn antarctica_is_ice() {
        // Mitten in Antarktika
        assert_eq!(sample_class(rad(-85.0), rad(0.0)), Class::Ice);
    }

    #[test]
    fn pacific_is_deep_sea() {
        // Mitten im Pazifik
        let c = sample_class(rad(0.0), rad(-150.0));
        assert!(matches!(c, Class::DeepSea | Class::Sea), "got {:?}", c);
    }

    #[test]
    fn sahara_is_land() {
        // Sahara: ~20°N, 10°E — sollte Land sein (Flatland oder Upland)
        let c = sample_class(rad(20.0), rad(10.0));
        assert!(
            matches!(c, Class::Flatland | Class::Upland),
            "Sahara should be land, got {:?}",
            c
        );
    }

    #[test]
    fn class_distribution_covers_multiple_classes() {
        use std::collections::HashSet;
        let mut seen: HashSet<Class> = HashSet::new();
        for lat_deg in -60..=60 {
            for lon_deg in (-180..180).step_by(2) {
                seen.insert(sample_class(rad(lat_deg as f64), rad(lon_deg as f64)));
            }
        }
        assert!(seen.contains(&Class::DeepSea) || seen.contains(&Class::Sea), "Wasser fehlt: {:?}", seen);
        assert!(seen.contains(&Class::Flatland), "Flatland fehlt: {:?}", seen);
        assert!(seen.len() >= 4, "Zu wenig Klassen: {:?}", seen);
    }

    #[test]
    fn sample_lights_zero_on_open_ocean() {
        // Mitten im Pazifik — keine Lichter
        assert_eq!(sample_lights(rad(0.0), rad(-150.0)), 0);
    }

    #[test]
    fn tokyo_region_has_lights() {
        // Tokio: ~35.7°N, 139.7°E — sollte hell sein
        let v = sample_lights(rad(35.7), rad(139.7));
        assert!(v > 50, "Tokio expected bright, got {}", v);
    }

    #[test]
    fn lights_have_distribution() {
        let mut nonzero = 0;
        let mut total = 0;
        for lat_deg in -50..=50 {
            for lon_deg in (-180..180).step_by(2) {
                total += 1;
                if sample_lights(rad(lat_deg as f64), rad(lon_deg as f64)) > 0 {
                    nonzero += 1;
                }
            }
        }
        assert!(nonzero > 50, "Kaum Lichter: {}/{}", nonzero, total);
        assert!(nonzero < total / 2, "Zu viele Lichter: {}/{}", nonzero, total);
    }

    #[test]
    fn clouds_return_some_coverage_and_some_clear_sky() {
        let mut covered = 0;
        let mut clear = 0;
        for lat_deg in (-80..80).step_by(5) {
            for lon_deg in (-180..180).step_by(5) {
                let c = sample_clouds(rad(lat_deg as f64), rad(lon_deg as f64));
                if c > 30 { covered += 1; } else { clear += 1; }
            }
        }
        assert!(covered > 50 && clear > 50, "covered={}, clear={}", covered, clear);
    }
}
