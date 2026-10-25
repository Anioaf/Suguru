use std::{collections::BTreeMap, fmt, time::Duration};

use super::{fixed::Fixed, hash::StateHasher, Tag};

const START_YEAR: u64 = 1936;
const TICKS_PER_DAY: u64 = 24;
const DAYS_PER_YEAR: u64 = 365;
const MONTH_DAYS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const TICK_MICROS: [u64; 5] = [200_000, 100_000, 50_000, 25_000, 10_000];
const MAX_SPEED: u8 = TICK_MICROS.len() as u8;
const DEFAULT_SPEED: u8 = 3;
const MAX_TICKS_PER_FRAME: u32 = 16;
const INCOME_PER_PROVINCE: Fixed = Fixed::from_ratio(1, 100);

#[derive(Clone, Copy, Debug)]
pub enum Command {
    TogglePause,
    SetSpeed(u8),
}

#[derive(Default)]
struct Country {
    provinces: u32,
    treasury: Fixed,
}

pub struct GameDate {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
}

impl fmt::Display for GameDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{} {:02}:00", self.year, self.month, self.day, self.hour)
    }
}

pub struct Simulation {
    tick: u64,
    paused: bool,
    speed: u8,
    countries: BTreeMap<Tag, Country>,
    accum_us: u64,
    pending: Vec<Command>,
}

impl Simulation {
    pub fn new(owners: &[Option<Tag>]) -> Self {
        let mut countries: BTreeMap<Tag, Country> = BTreeMap::new();
        for tag in owners.iter().flatten() {
            countries.entry(*tag).or_default().provinces += 1;
        }
        Self {
            tick: 0,
            paused: false,
            speed: DEFAULT_SPEED,
            countries,
            accum_us: 0,
            pending: Vec::new(),
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn date(&self) -> GameDate {
        let mut day = self.tick / TICKS_PER_DAY;
        let hour = self.tick % TICKS_PER_DAY;
        let year = START_YEAR + day / DAYS_PER_YEAR;
        day %= DAYS_PER_YEAR;
        let mut month = 0;
        while day >= MONTH_DAYS[month] {
            day -= MONTH_DAYS[month];
            month += 1;
        }
        GameDate { year, month: month as u64 + 1, day: day + 1, hour }
    }

    pub fn submit(&mut self, cmd: Command) {
        self.pending.push(cmd);
    }

    pub fn advance(&mut self, dt: Duration) {
        self.apply_pending();
        if self.paused {
            self.accum_us = 0;
            return;
        }
        self.accum_us += dt.as_micros().min(u64::MAX as u128) as u64;
        let step_us = TICK_MICROS[self.speed as usize - 1];
        let mut done = 0;
        while self.accum_us >= step_us && done < MAX_TICKS_PER_FRAME {
            self.accum_us -= step_us;
            self.step();
            done += 1;
        }
        if done == MAX_TICKS_PER_FRAME {
            self.accum_us = 0;
        }
    }

    pub fn hash_into(&self, h: &mut StateHasher) {
        h.u64(self.tick);
        h.u8(self.paused as u8);
        h.u8(self.speed);
        for (tag, c) in &self.countries {
            h.bytes(tag.as_str().as_bytes());
            h.u64(c.provinces as u64);
            h.i64(c.treasury.raw());
        }
    }

    fn apply_pending(&mut self) {
        for cmd in self.pending.drain(..) {
            log::info!("tick {}: {:?}", self.tick, cmd);
            match cmd {
                Command::TogglePause => self.paused = !self.paused,
                Command::SetSpeed(s) => self.speed = s.clamp(1, MAX_SPEED),
            }
        }
    }

    fn step(&mut self) {
        for c in self.countries.values_mut() {
            c.treasury += INCOME_PER_PROVINCE.mul_int(c.provinces as i64);
        }
        self.tick += 1;
    }
}