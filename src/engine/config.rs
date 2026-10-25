use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub title: String,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub fps_cap: Option<u32>,
    pub content_root: PathBuf,
    pub language: String,
    pub font_candidates: Vec<PathBuf>,
    pub font_px: f32,
}

fn default_root() -> PathBuf {
    if let Some(p) = std::env::var_os("SUGURU_CONTENT") {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "Suguru".into(),
            fullscreen: false,
            width: 1600,
            height: 900,
            vsync: std::env::var("SUGURU_VSYNC").is_ok_and(|v| v == "1"),
            fps_cap: std::env::var("SUGURU_FPS").ok().and_then(|v| v.parse().ok()).or(Some(300)),
            content_root: default_root(),
            language: "russian".into(),
            font_px: 64.0,
            font_candidates: [
                "font/main.ttf",
                "C:/Windows/Fonts/arial.ttf",
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        }
    }
}

impl Config {
    pub fn find_font(&self) -> Option<PathBuf> {
        let direct = self
            .font_candidates
            .iter()
            .flat_map(|p| [self.content_root.join(p), p.clone()])
            .find(|p| p.is_file());
        if direct.is_some() {
            return direct;
        }
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.content_root.join("font"))
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            })
            .collect();
        found.sort();
        found.into_iter().next()
    }
}