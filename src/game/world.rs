use super::{map::MapData, parsers, Tag, WORLD_HEIGHT, WORLD_WIDTH};
use crate::engine::Config;
use anyhow::{ensure, Result};
use glam::Vec2;
use rustc_hash::FxHashMap;

pub struct CountryLabel {
    #[allow(dead_code)]
    pub tag: Tag,
    pub name: String,
    pub pos: Vec2,
    pub size: f32,
}

#[derive(Default)]
pub struct Selection {
    pub province: Option<u32>,
    pub owner: Option<Tag>,
}

pub struct World {
    pub map: MapData,
    pub country_colors: FxHashMap<Tag, [u8; 3]>,
    pub labels: Vec<CountryLabel>,
    pub selection: Selection,
}

fn or_default<T: Default>(r: Result<T>, what: &str) -> T {
    r.unwrap_or_else(|e| {
        log::error!("{what}: {e:#}");
        T::default()
    })
}

fn load_owners(cfg: &Config) -> FxHashMap<u32, Tag> {
    for dir in ["history/states", "history/state"] {
        let path = cfg.path(dir);
        if path.is_dir() {
            return or_default(parsers::load_states(&path), "states");
        }
    }
    log::error!("папка history/states не найдена, владельцы провинций не загружены");
    FxHashMap::default()
}

impl World {
    pub fn load(cfg: &Config) -> Result<Self> {
        let ((defs, owners), (country_colors, (parties, loc))) = rayon::join(
            || {
                rayon::join(
                    || parsers::load_definitions(&cfg.path("map/definition.csv")),
                    || load_owners(cfg),
                )
            },
            || {
                rayon::join(
                    || or_default(parsers::load_country_colors(&cfg.path("common/countries/colors.txt")), "country colors"),
                    || {
                        rayon::join(
                            || or_default(parsers::load_country_parties(&cfg.path("history/countries")), "country history"),
                            || or_default(parsers::load_localization(&cfg.path("localisation"), &cfg.language), "localisation"),
                        )
                    },
                )
            },
        );
        let defs = defs?;

        let map = MapData::load(&cfg.path("map/provinces.bmp"), &defs, &owners)?;
        ensure!(
            (map.width as f32 / map.height as f32 - WORLD_WIDTH / WORLD_HEIGHT).abs() < 1e-3,
            "размер карты {}x{} не соответствует WORLD_WIDTH/WORLD_HEIGHT",
            map.width,
            map.height
        );
        let labels = Self::build_labels(&map, &parties, &loc);
        log::info!("world loaded: {} provinces, {} labels", defs.len(), labels.len());

        Ok(Self { map, country_colors, labels, selection: Selection::default() })
    }

    fn build_labels(
        map: &MapData,
        parties: &FxHashMap<Tag, String>,
        loc: &FxHashMap<String, String>,
    ) -> Vec<CountryLabel> {
        let mut labels: Vec<CountryLabel> = map
            .country_anchors()
            .into_iter()
            .filter(|a| a.total_pixels >= 100)
            .map(|a| {
                let tag = a.tag;
                let u = a.x / map.width as f32;
                let v = (a.y + 0.5) / map.height as f32;

                let party_key = parties.get(&tag).map(|p| format!("{tag}_{p}"));
                let name = party_key
                    .as_ref()
                    .and_then(|k| loc.get(k))
                    .or_else(|| loc.get(tag.as_str()))
                    .cloned()
                    .unwrap_or_else(|| tag.to_string());

                CountryLabel {
                    tag,
                    name,
                    pos: Vec2::new(u * WORLD_WIDTH, v * WORLD_HEIGHT),
                    size: (0.00001 * (a.main_pixels as f32).sqrt()).clamp(0.0008, 0.0030),
                }
            })
            .collect();
        labels.sort_by_key(|l| l.tag);
        labels
    }

    pub fn pick(&mut self, p: Vec2) {
        let u = (p.x / WORLD_WIDTH).rem_euclid(1.0);
        let v = p.y / WORLD_HEIGHT;
        if !(0.0..=1.0).contains(&v) {
            return;
        }
        let id = self.map.province_at(u, v);
        self.selection.province = id;
        self.selection.owner = id.and_then(|i| self.map.owner_of(i));
        match id {
            Some(id) => log::info!(
                "selected province {id}, owner {}",
                self.selection.owner.map_or("None".to_string(), |t| t.to_string())
            ),
            None => log::info!("selection cleared"),
        }
    }
}