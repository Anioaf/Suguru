use std::ops::{Add, AddAssign};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Fixed(i64);

impl Fixed {
    pub const fn from_ratio(num: i64, den: i64) -> Self {
        Self((((num as i128) << 32) / den as i128) as i64)
    }

    pub fn mul_int(self, n: i64) -> Self {
        Self(self.0.saturating_mul(n))
    }

    pub fn raw(self) -> i64 {
        self.0
    }
}

impl Add for Fixed {
    type Output = Self;

    fn add(self, o: Self) -> Self {
        Self(self.0.saturating_add(o.0))
    }
}

impl AddAssign for Fixed {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}