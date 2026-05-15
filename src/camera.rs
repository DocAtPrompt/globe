//! Kamera-State: lat/lon/distance.
//!
//! Höhere App-Modi (Freeze, Auto-Rotation, Render-Mode, Sun-Delta) liegen im
//! `app`-Modul — die Camera kennt nur ihre Geometrie.

use crate::constants::{ZOOM_DEFAULT, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP_IN, ZOOM_STEP_OUT};

pub const MAX_LAT_DEG: f64 = 89.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Breitengrad in Grad, ±89°
    pub lat_deg: f64,
    /// Längengrad in Grad, [-180, 180]
    pub lon_deg: f64,
    /// Abstand vom Erdmittelpunkt in Erdradien, [ZOOM_MIN, ZOOM_MAX]
    pub distance: f64,
}

impl Camera {
    pub fn new(lat_deg: f64, lon_deg: f64) -> Self {
        let mut c = Self {
            lat_deg,
            lon_deg,
            distance: ZOOM_DEFAULT,
        };
        c.clamp();
        c
    }

    pub fn rotate(&mut self, d_lat_deg: f64, d_lon_deg: f64) {
        self.lat_deg += d_lat_deg;
        self.lon_deg += d_lon_deg;
        self.clamp();
    }

    pub fn jump_to(&mut self, lat_deg: f64, lon_deg: f64) {
        self.lat_deg = lat_deg;
        self.lon_deg = lon_deg;
        self.clamp();
    }

    pub fn zoom_in(&mut self) {
        self.distance = (self.distance * ZOOM_STEP_IN).max(ZOOM_MIN);
    }

    pub fn zoom_out(&mut self) {
        self.distance = (self.distance * ZOOM_STEP_OUT).min(ZOOM_MAX);
    }

    pub fn zoom_reset(&mut self) {
        self.distance = ZOOM_DEFAULT;
    }

    fn clamp(&mut self) {
        self.lat_deg = self.lat_deg.clamp(-MAX_LAT_DEG, MAX_LAT_DEG);
        // Lon wrap to [-180, 180]
        let mut l = ((self.lon_deg + 180.0).rem_euclid(360.0)) - 180.0;
        if l == -180.0 {
            l = 180.0;
        }
        self.lon_deg = l;
        self.distance = self.distance.clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn new_uses_default_zoom() {
        let c = Camera::new(0.0, 0.0);
        assert_eq!(c.distance, ZOOM_DEFAULT);
        assert_eq!(c.lat_deg, 0.0);
        assert_eq!(c.lon_deg, 0.0);
    }

    #[test]
    fn lat_clamped_at_pm89() {
        let mut c = Camera::new(0.0, 0.0);
        c.rotate(200.0, 0.0);
        assert_eq!(c.lat_deg, MAX_LAT_DEG);
        c.rotate(-500.0, 0.0);
        assert_eq!(c.lat_deg, -MAX_LAT_DEG);
    }

    #[test]
    fn lon_wraps_at_180() {
        let mut c = Camera::new(0.0, 170.0);
        c.rotate(0.0, 20.0); // → 190 → -170
        assert!(close(c.lon_deg, -170.0, 1e-9));
        c.rotate(0.0, -40.0); // → -210 → 150
        assert!(close(c.lon_deg, 150.0, 1e-9));
    }

    #[test]
    fn zoom_in_clamps_to_min() {
        let mut c = Camera::new(0.0, 0.0);
        for _ in 0..50 {
            c.zoom_in();
        }
        assert!((c.distance - ZOOM_MIN).abs() < 1e-9);
    }

    #[test]
    fn zoom_out_clamps_to_max() {
        let mut c = Camera::new(0.0, 0.0);
        for _ in 0..50 {
            c.zoom_out();
        }
        assert!((c.distance - ZOOM_MAX).abs() < 1e-9);
    }

    #[test]
    fn zoom_step_ratio_correct() {
        let mut c = Camera::new(0.0, 0.0);
        c.zoom_in();
        assert!(close(c.distance, ZOOM_DEFAULT * ZOOM_STEP_IN, 1e-9));
    }

    #[test]
    fn zoom_reset_sets_default() {
        let mut c = Camera::new(0.0, 0.0);
        c.zoom_out();
        c.zoom_out();
        c.zoom_reset();
        assert_eq!(c.distance, ZOOM_DEFAULT);
    }

    #[test]
    fn jump_to_overrides_position() {
        let mut c = Camera::new(10.0, 20.0);
        c.jump_to(48.21, 16.37);
        assert!(close(c.lat_deg, 48.21, 1e-9));
        assert!(close(c.lon_deg, 16.37, 1e-9));
    }

    #[test]
    fn jump_to_clamps_extreme_values() {
        let mut c = Camera::new(0.0, 0.0);
        c.jump_to(95.0, 200.0); // beide außerhalb
        assert_eq!(c.lat_deg, MAX_LAT_DEG);
        assert!(close(c.lon_deg, -160.0, 1e-9));
    }

    #[test]
    fn new_clamps_input() {
        let c = Camera::new(100.0, 250.0);
        assert_eq!(c.lat_deg, MAX_LAT_DEG);
        assert!(close(c.lon_deg, -110.0, 1e-9));
    }
}
