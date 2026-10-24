use super::{parsers::pack_rgb, Tag};
use anyhow::{Context, Result};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::path::Path;

pub const NO_PROVINCE: u32 = u32::MAX;

/// Растровая карта провинций. Строка 0 = НИЗ карты (изображение перевернуто при загрузке).
pub struct MapData {
    pub width: u32,
    pub height: u32,
    /// RGB провинций (для GPU и для подсветки выбранного цвета)
    pub rgb: Vec<u8>,
    /// id провинции на каждый пиксель — пикинг и подсчеты становятся O(1) без хэш-таблиц
    pub province_ids: Vec<u32>,
    /// владелец по id провинции
    pub owner: Vec<Option<Tag>>,
}

impl MapData {
    pub fn load(path: &Path, defs: &FxHashMap<u32, u32>, owners: &FxHashMap<u32, Tag>) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("provinces map {}", path.display()))?
            .flipv()
            .to_rgb8();
        let (width, height) = img.dimensions();
        let rgb = img.into_raw();

        let province_ids: Vec<u32> = rgb
            .par_chunks_exact(3)
            .map(|p| defs.get(&pack_rgb(p[0], p[1], p[2])).copied().unwrap_or(NO_PROVINCE))
            .collect();

        let max_id = defs.values().copied().max().unwrap_or(0) as usize;
        let mut owner = vec![None; max_id + 1];
        for (&id, &tag) in owners {
            if let Some(slot) = owner.get_mut(id as usize) {
                *slot = Some(tag);
            }
        }
        Ok(Self { width, height, rgb, province_ids, owner })
    }

    /// (u, v) в [0,1] → (id провинции, исходный цвет пикселя)
    pub fn province_at(&self, u: f32, v: f32) -> (Option<u32>, [u8; 3]) {
        let (w, h) = (self.width as usize, self.height as usize);
        let x = ((u * w as f32) as usize).min(w - 1);
        let y = ((v * h as f32) as usize).min(h - 1);
        let i = y * w + x;
        let id = self.province_ids[i];
        (
            (id != NO_PROVINCE).then_some(id),
            [self.rgb[i * 3], self.rgb[i * 3 + 1], self.rgb[i * 3 + 2]],
        )
    }

    pub fn owner_of(&self, id: u32) -> Option<Tag> {
        self.owner.get(id as usize).copied().flatten()
    }

    /// Карта цветов владельцев (для текстуры stateMap).
    pub fn owner_color_image(&self, colors: &FxHashMap<Tag, [u8; 3]>) -> Vec<u8> {
        let palette: Vec<[u8; 3]> = self
            .owner
            .iter()
            .map(|o| o.and_then(|t| colors.get(&t).copied()).unwrap_or([0; 3]))
            .collect();
        let mut out = vec![0u8; self.province_ids.len() * 3];
        out.par_chunks_exact_mut(3)
            .zip(self.province_ids.par_iter())
            .for_each(|(px, &id)| {
                if let Some(c) = palette.get(id as usize) {
                    px.copy_from_slice(c);
                }
            });
        out
    }

    /// Сумма координат и число пикселей по каждой стране (параллельно по строкам).
    pub fn country_centroids(&self) -> FxHashMap<Tag, (f64, f64, u64)> {
        let w = self.width as usize;
        self.province_ids
            .par_chunks(w)
            .enumerate()
            .fold(FxHashMap::default, |mut acc: FxHashMap<Tag, (f64, f64, u64)>, (y, row)| {
                for (x, &id) in row.iter().enumerate() {
                    if let Some(tag) = self.owner.get(id as usize).copied().flatten() {
                        let e = acc.entry(tag).or_insert((0.0, 0.0, 0));
                        e.0 += x as f64;
                        e.1 += y as f64;
                        e.2 += 1;
                    }
                }
                acc
            })
            .reduce(FxHashMap::default, |mut a, b| {
                for (k, v) in b {
                    let e = a.entry(k).or_insert((0.0, 0.0, 0));
                    e.0 += v.0;
                    e.1 += v.1;
                    e.2 += v.2;
                }
                a
            })
    }
}
