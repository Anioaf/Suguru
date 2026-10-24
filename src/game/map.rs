use super::{parsers::pack_rgb, Tag};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;

pub const NO_PROVINCE: u16 = u16::MAX;
pub const PALETTE_SIDE: u32 = 256;

pub struct CountryAnchor {
    pub tag: Tag,
    pub x: f32,
    pub y: f32,
    pub main_pixels: u64,
    pub total_pixels: u64,
}

pub struct MapData {
    pub width: u32,
    pub height: u32,
    pub province_ids: Vec<u16>,
    pub owner: Vec<Option<Tag>>,
}

#[derive(Clone, Copy, Default)]
struct Acc {
    n: u64,
    y: f64,
    c: f64,
    s: f64,
}

impl Acc {
    fn add(&mut self, o: Acc) {
        self.n += o.n;
        self.y += o.y;
        self.c += o.c;
        self.s += o.s;
    }
}

fn link(set: &mut FxHashSet<(u16, u16)>, a: u16, b: u16, owner: &[Option<Tag>]) {
    if a == b {
        return;
    }
    let (Some(Some(oa)), Some(Some(ob))) = (owner.get(a as usize), owner.get(b as usize)) else { return };
    if oa == ob {
        set.insert((a.min(b), a.max(b)));
    }
}

fn find(p: &mut [u32], mut x: u32) -> u32 {
    while p[x as usize] != x {
        let g = p[p[x as usize] as usize];
        p[x as usize] = g;
        x = g;
    }
    x
}

impl MapData {
    pub fn load(path: &Path, defs: &FxHashMap<u32, u32>, owners: &FxHashMap<u32, Tag>) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("provinces map {}", path.display()))?
            .flipv()
            .to_rgb8();
        let (width, height) = img.dimensions();
        let (w, h) = (width as usize, height as usize);
        let rgb = img.into_raw();

        let max_id = defs.values().copied().max().unwrap_or(0);
        if max_id >= NO_PROVINCE as u32 {
            bail!("слишком много провинций для u16: максимальный id {max_id}");
        }

        let mut province_ids = vec![NO_PROVINCE; w * h];
        province_ids
            .par_chunks_mut(w)
            .zip(rgb.par_chunks(w * 3))
            .for_each(|(out, row)| {
                let mut last = (u32::MAX, NO_PROVINCE);
                for (d, p) in out.iter_mut().zip(row.chunks_exact(3)) {
                    let c = pack_rgb(p[0], p[1], p[2]);
                    if c != last.0 {
                        last = (c, defs.get(&c).map_or(NO_PROVINCE, |&i| i as u16));
                    }
                    *d = last.1;
                }
            });
        drop(rgb);

        let mut owner = vec![None; max_id as usize + 1];
        for (&id, &tag) in owners {
            if let Some(slot) = owner.get_mut(id as usize) {
                *slot = Some(tag);
            }
        }
        Ok(Self { width, height, province_ids, owner })
    }

    pub fn province_at(&self, u: f32, v: f32) -> Option<u32> {
        let (w, h) = (self.width as usize, self.height as usize);
        let x = ((u * w as f32) as usize).min(w - 1);
        let y = ((v * h as f32) as usize).min(h - 1);
        let id = self.province_ids[y * w + x];
        (id != NO_PROVINCE).then_some(id as u32)
    }

    pub fn owner_of(&self, id: u32) -> Option<Tag> {
        self.owner.get(id as usize).copied().flatten()
    }

    pub fn owner_palette(&self, colors: &FxHashMap<Tag, [u8; 3]>) -> Vec<u8> {
        let mut out = vec![0u8; (PALETTE_SIDE * PALETTE_SIDE * 4) as usize];
        for (id, o) in self.owner.iter().enumerate() {
            if let Some(c) = o.and_then(|t| colors.get(&t)) {
                out[id * 4..id * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        out
    }

    pub fn country_anchors(&self) -> Vec<CountryAnchor> {
        let w = self.width as usize;
        let h = self.height as usize;
        let n = self.owner.len();
        let ids = &self.province_ids;
        let owner = &self.owner;
        let tau = std::f64::consts::TAU;
        let trig: Vec<(f64, f64)> = (0..w)
            .map(|x| {
                let a = x as f64 / w as f64 * tau;
                (a.cos(), a.sin())
            })
            .collect();

        let stats: Vec<Acc> = ids
            .par_chunks(w)
            .enumerate()
            .fold(
                || vec![Acc::default(); n],
                |mut acc, (y, row)| {
                    for (x, &id) in row.iter().enumerate() {
                        if let Some(a) = acc.get_mut(id as usize) {
                            a.n += 1;
                            a.y += y as f64;
                            a.c += trig[x].0;
                            a.s += trig[x].1;
                        }
                    }
                    acc
                },
            )
            .reduce(
                || vec![Acc::default(); n],
                |mut a, b| {
                    for (l, r) in a.iter_mut().zip(b) {
                        l.add(r);
                    }
                    a
                },
            );

        let edges: FxHashSet<(u16, u16)> = (0..h)
            .into_par_iter()
            .fold(FxHashSet::<(u16, u16)>::default, |mut set, y| {
                let row = &ids[y * w..(y + 1) * w];
                let below = (y + 1 < h).then(|| &ids[(y + 1) * w..(y + 2) * w]);
                for x in 0..w {
                    let a = row[x];
                    link(&mut set, a, row[if x + 1 == w { 0 } else { x + 1 }], owner);
                    if let Some(b) = below {
                        link(&mut set, a, b[x], owner);
                    }
                }
                set
            })
            .reduce(FxHashSet::default, |mut a, b| {
                a.extend(b);
                a
            });

        let mut parent: Vec<u32> = (0..n as u32).collect();
        for (a, b) in edges {
            let ra = find(&mut parent, a as u32);
            let rb = find(&mut parent, b as u32);
            if ra != rb {
                parent[ra as usize] = rb;
            }
        }

        let mut comps: FxHashMap<u32, (Tag, Acc)> = FxHashMap::default();
        for (id, o) in owner.iter().enumerate() {
            let (Some(tag), true) = (o, stats[id].n > 0) else { continue };
            let root = find(&mut parent, id as u32);
            comps.entry(root).or_insert((*tag, Acc::default())).1.add(stats[id]);
        }

        let mut by_tag: FxHashMap<Tag, (Acc, u64)> = FxHashMap::default();
        for (_, (tag, acc)) in comps {
            let e = by_tag.entry(tag).or_insert((Acc::default(), 0));
            e.1 += acc.n;
            if acc.n > e.0.n {
                e.0 = acc;
            }
        }

        by_tag
            .into_iter()
            .map(|(tag, (a, total))| {
                let ang = a.s.atan2(a.c).rem_euclid(tau);
                CountryAnchor {
                    tag,
                    x: (ang / tau * w as f64) as f32,
                    y: (a.y / a.n as f64) as f32,
                    main_pixels: a.n,
                    total_pixels: total,
                }
            })
            .collect()
    }
}