pub mod map;
pub mod parsers;
pub mod world;

use std::fmt;

pub use world::World;

/// Размеры мира в мировых единицах (карта 5632×2048 px → 5.632×2.048).
pub const WORLD_WIDTH: f32 = 5.632;
pub const WORLD_HEIGHT: f32 = 2.048;

/// Трехбуквенный тег страны (GER, SOV, ...). Copy, без аллокаций, быстро хэшируется.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Tag([u8; 3]);

impl Tag {
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        (b.len() == 3 && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
            .then(|| Tag([b[0], b[1], b[2]]))
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
