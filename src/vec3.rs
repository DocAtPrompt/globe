#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V3(pub f64, pub f64, pub f64);

impl V3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        V3(x, y, z)
    }
    // Diese Methoden teilen ihre Namen mit `std::ops::{Add, Sub, Mul}` —
    // clippy flaggt das, aber die Methoden-Form ist hier bewusst leichtgewichtig
    // gewählt (Inline-Math im Render-Pfad, kein `impl Add for V3` mit Borrow-
    // Implikationen).
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, b: V3) -> V3 {
        V3(self.0 + b.0, self.1 + b.1, self.2 + b.2)
    }
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, b: V3) -> V3 {
        V3(self.0 - b.0, self.1 - b.1, self.2 - b.2)
    }
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, s: f64) -> V3 {
        V3(self.0 * s, self.1 * s, self.2 * s)
    }
    pub fn dot(self, b: V3) -> f64 {
        self.0 * b.0 + self.1 * b.1 + self.2 * b.2
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l == 0.0 {
            self
        } else {
            self.mul(1.0 / l)
        }
    }
    pub fn cross(self, b: V3) -> V3 {
        V3(
            self.1 * b.2 - self.2 * b.1,
            self.2 * b.0 - self.0 * b.2,
            self.0 * b.1 - self.1 * b.0,
        )
    }
}

/// Rotation um die Y-Achse (in Welt-Koord = Erdachse).
pub fn rotate_y(p: V3, angle_rad: f64) -> V3 {
    let c = angle_rad.cos();
    let s = angle_rad.sin();
    V3(p.0 * c + p.2 * s, p.1, -p.0 * s + p.2 * c)
}

pub fn from_lat_lon(lat: f64, lon: f64) -> V3 {
    let cl = lat.cos();
    V3(cl * lon.cos(), lat.sin(), cl * lon.sin())
}

pub fn to_lat_lon(p: V3) -> (f64, f64) {
    let n = p.norm();
    let lat = n.1.clamp(-1.0, 1.0).asin();
    let lon = n.2.atan2(n.0);
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn basic_arith() {
        let a = V3::new(1.0, 2.0, 3.0);
        let b = V3::new(4.0, 5.0, 6.0);
        assert_eq!(a.add(b), V3::new(5.0, 7.0, 9.0));
        assert_eq!(a.sub(b), V3::new(-3.0, -3.0, -3.0));
        assert_eq!(a.mul(2.0), V3::new(2.0, 4.0, 6.0));
        assert_eq!(a.dot(b), 32.0);
    }

    #[test]
    fn norm_unit_length() {
        let v = V3::new(3.0, 4.0, 0.0);
        let n = v.norm();
        assert!(close(n.len(), 1.0, 1e-12));
    }

    #[test]
    fn lat_lon_roundtrip() {
        for (lat, lon) in [
            (0.0, 0.0),
            (PI / 4.0, PI / 3.0),
            (-PI / 6.0, -PI / 2.0),
            (1.2, 2.5),
        ] {
            let p = from_lat_lon(lat, lon);
            let (lat2, lon2) = to_lat_lon(p);
            assert!(close(lat, lat2, 1e-10), "lat: {} vs {}", lat, lat2);
            assert!(close(lon, lon2, 1e-10), "lon: {} vs {}", lon, lon2);
        }
    }

    #[test]
    fn from_lat_lon_zero_is_x_axis() {
        let p = from_lat_lon(0.0, 0.0);
        assert!(close(p.0, 1.0, 1e-12));
        assert!(close(p.1, 0.0, 1e-12));
        assert!(close(p.2, 0.0, 1e-12));
    }

    #[test]
    fn from_lat_lon_north_pole_is_y_axis() {
        let p = from_lat_lon(PI / 2.0, 0.0);
        assert!(close(p.1, 1.0, 1e-12));
    }

    #[test]
    fn cross_right_handed() {
        let x = V3::new(1.0, 0.0, 0.0);
        let y = V3::new(0.0, 1.0, 0.0);
        let z = V3::new(0.0, 0.0, 1.0);
        let r = x.cross(y);
        assert!(close(r.0, z.0, 1e-12));
        assert!(close(r.1, z.1, 1e-12));
        assert!(close(r.2, z.2, 1e-12));
    }

    #[test]
    fn rotate_y_quarter_turn() {
        let p = V3::new(1.0, 0.0, 0.0);
        let r = rotate_y(p, PI / 2.0);
        assert!(close(r.0, 0.0, 1e-12));
        assert!(close(r.2, -1.0, 1e-12));
    }
}
