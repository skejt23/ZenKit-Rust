use glam::{Mat4, Vec3};
use std::collections::HashSet;
use winit::keyboard::KeyCode;

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    home: (Vec3, f32, f32),
}

impl Camera {
    pub fn new(position: Vec3, target: Vec3, speed: f32) -> Self {
        let d = (target - position).normalize_or_zero();
        let mut c = Self {
            position,
            yaw: d.x.atan2(-d.z),
            pitch: d.y.asin(),
            speed,
            home: (position, 0.0, 0.0),
        };
        c.save_home();
        c
    }
    pub fn save_home(&mut self) {
        self.home = (self.position, self.yaw, self.pitch);
    }
    pub fn reset(&mut self) {
        (self.position, self.yaw, self.pitch) = self.home;
    }
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }
    pub fn look(&mut self, x: f64, y: f64) {
        self.yaw += x as f32 * 0.002;
        self.pitch = (self.pitch - y as f32 * 0.002).clamp(-1.55, 1.55);
    }
    pub fn step(&mut self, keys: &HashSet<KeyCode>, dt: f32) {
        let axis = |positive, negative| {
            f32::from(keys.contains(&positive)) - f32::from(keys.contains(&negative))
        };
        let forward = self.forward();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let direction = forward * axis(KeyCode::KeyW, KeyCode::KeyS)
            + right * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::KeyE, KeyCode::KeyQ);
        let fast = if keys.contains(&KeyCode::ShiftLeft) || keys.contains(&KeyCode::ShiftRight) {
            5.0
        } else {
            1.0
        };
        self.position += direction.normalize_or_zero() * self.speed * fast * dt.min(0.1);
    }
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(70_f32.to_radians(), aspect, 0.1, 20_000.0)
            * Mat4::look_to_rh(self.position, self.forward(), Vec3::Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagonal_flight_has_the_same_speed_and_mouse_changes_direction() {
        let mut c = Camera::new(Vec3::ZERO, -Vec3::Z, 10.0);
        c.step(&HashSet::from([KeyCode::KeyW, KeyCode::KeyD]), 0.1);
        assert!((c.position.length() - 1.0).abs() < 0.0001);
        c.look(100.0, -100_000.0);
        assert!(c.forward().is_finite());
        assert!(c.forward().x > 0.0);
        c.reset();
        assert_eq!(c.position, Vec3::ZERO);
    }
}
