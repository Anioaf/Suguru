use glam::{Mat4, Vec2, Vec3};

const MIN_Z: f32 = 0.1;
const MAX_Z: f32 = 2.5;

#[derive(Clone, Copy, Default)]
pub struct CameraInput {
    pub pan: Vec2,
    pub scroll: f32,
}

pub struct Camera {
    pub position: Vec3,
    map_size: Vec2,
    target_z: f32,
}

impl Camera {
    pub fn new(map_size: Vec2) -> Self {
        Self { position: Vec3::new(map_size.x * 0.5, map_size.y * 0.5, 1.0), map_size, target_z: 1.0 }
    }

    pub fn update(&mut self, dt: f32, input: CameraInput) {
        let speed = 2.0 * (self.position.z / MAX_Z);
        self.position.x += input.pan.x * speed * dt;
        self.position.y += input.pan.y * speed * dt;

        self.target_z = (self.target_z - input.scroll * 0.1 * self.target_z).clamp(MIN_Z, MAX_Z);
        let k = 1.0 - (-12.0 * dt).exp();
        self.position.z += (self.target_z - self.position.z) * k;

        self.position.y = self.position.y.clamp(0.0, self.map_size.y);
        self.position.x = self.position.x.rem_euclid(self.map_size.x);
    }

    pub fn view(&self) -> Mat4 {
        let p = self.position;
        Mat4::look_at_rh(p, Vec3::new(p.x, p.y, 0.0), Vec3::Y)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(45f32.to_radians(), aspect, 0.01, 100.0)
    }

    pub fn screen_to_world(&self, ndc: Vec2, aspect: f32) -> Option<Vec2> {
        let inv = (self.proj(aspect) * self.view()).inverse();
        let near = inv.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        let dz = near.z - far.z;
        if dz.abs() < 1e-8 {
            return None;
        }
        let p = near + (far - near) * (near.z / dz);
        Some(Vec2::new(p.x, p.y))
    }
}