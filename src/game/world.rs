use super::{map::MapData, parsers, Tag, WORLD_HEIGHT, WORLD_WIDTH};
use crate::engine::Config;
use anyhow::Result;
use glam::{Vec2, Vec3};
use rustc_hash::FxHashMap;

pub struct CountryLabel {
    #[allow(dead_code)]
    pub tag: Tag,
    pub name: String,
    pub pos: Vec2,
    /// мировых единиц на пиксель шрифта
    pub size: f32,
}

#[derive(Default)]
pub struct Selection {
    pub province: Option<u32>,
    pub owner: Option<Tag>,
    pub rgb: Vec3,
}

/// Игровое состояние мира (данные карты, страны, выбор). Ничего не знает про OpenGL.
pub struct World {
    pub map: MapData,
    pub country_colors: FxHashMap<Tag, [u8; 3]>,
    pub labels: Vec<CountryLabel>,
    pub selection: Selection,
}

fn or_default<T: Default>(r: Result<T>, what: &str) -> T {
    r.unwrap_or_else(|e| {
        log::warn!("{what}: {e:#}");
        T::default()
    })
}

impl World {
    pub fn load(cfg: &Config) -> Result<Self> {
        let defs = parsers::load_definitions(&cfg.path("map/definition.csv"))?;
        let country_colors = or_default(
            parsers::load_country_colors(&cfg.path("common/countries/colors.txt")),
            "country colors",
        );
        let owners = or_default(parsers::load_states(&cfg.path("history/state")), "states");
        let parties = or_default(parsers::load_country_parties(&cfg.path("history/countries")), "country history");
        let loc = or_default(parsers::load_localization(&cfg.path("localisation"), &cfg.language), "localisation");

        let map = MapData::load(&cfg.path("map/provinces.bmp"), &defs, &owners)?;
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
            .country_centroids()
            .into_iter()
            .filter(|(_, (_, _, n))| *n >= 100)
            .map(|(tag, (sx, sy, n))| {
                let u = (sx / n as f64 / map.width as f64) as f32;
                let v = (sy / n as f64 / map.height as f64) as f32;

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
                    size: (0.00001 * (n as f32).sqrt()).clamp(0.0008, 0.0030),
                }
            })
            .collect();
        labels.sort_by_key(|l| l.tag);
        labels
    }

    /// Выбор провинции по точке в мировых координатах (плоскость z=0).
    pub fn pick(&mut self, p: Vec2) {
        let u = (p.x / WORLD_WIDTH).rem_euclid(1.0); // горизонтальный wrap
        let v = p.y / WORLD_HEIGHT;
        if !(0.0..=1.0).contains(&v) {
            return;
        }
        let (id, rgb) = self.map.province_at(u, v);
        self.selection.rgb = Vec3::new(rgb[0] as f32, rgb[1] as f32, rgb[2] as f32) / 255.0;
        if let Some(id) = id {
            self.selection.province = Some(id);
            self.selection.owner = self.map.owner_of(id);
            log::info!(
                "selected province {id}, owner {}",
                self.selection.owner.map_or("None".to_string(), |t| t.to_string())
            );
        }
    }
}
