use super::Tag;
use anyhow::{Context, Result};
use rayon::prelude::*;
use regex::Regex;
use rustc_hash::FxHashMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn read_text(p: &Path) -> Result<String> {
    let bytes = fs::read(p).with_context(|| format!("read {}", p.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn strip_comments(s: &str) -> String {
    s.lines().map(|l| l.split('#').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

fn files_with_ext(dir: &Path, ext: &str) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    v.sort();
    Ok(v)
}

pub fn load_definitions(path: &Path) -> Result<FxHashMap<u32, u32>> {
    let text = read_text(path)?;
    let mut map = FxHashMap::default();
    for line in text.lines() {
        let mut it = line.split(';');
        let (Some(id), Some(r), Some(g), Some(b)) = (it.next(), it.next(), it.next(), it.next()) else { continue };
        let (Ok(id), Ok(r), Ok(g), Ok(b)) =
            (id.trim().parse::<u32>(), r.trim().parse::<u8>(), g.trim().parse::<u8>(), b.trim().parse::<u8>())
        else {
            continue;
        };
        map.insert(pack_rgb(r, g, b), id);
    }
    Ok(map)
}

pub fn load_country_colors(path: &Path) -> Result<FxHashMap<Tag, [u8; 3]>> {
    let content = strip_comments(&read_text(path)?);
    let re = Regex::new(r"\b([A-Z0-9]{3})\b\s*=\s*\{[^}]*\bcolor\s*=\s*rgb\s*\{\s*(\d+)\s+(\d+)\s+(\d+)\s*\}")?;
    let mut out = FxHashMap::default();
    for c in re.captures_iter(&content) {
        let (Some(tag), Ok(r), Ok(g), Ok(b)) =
            (Tag::parse(&c[1]), c[2].parse::<u8>(), c[3].parse::<u8>(), c[4].parse::<u8>())
        else {
            continue;
        };
        out.insert(tag, [r, g, b]);
    }
    Ok(out)
}

pub fn load_states(dir: &Path) -> Result<FxHashMap<u32, Tag>> {
    let owner_re = Regex::new(r"\bowner\s*=\s*([A-Z0-9]+)")?;
    let prov_re = Regex::new(r"provinces\s*=\s*\{([^}]+)\}")?;
    let files = files_with_ext(dir, "txt")?;
    let parts: Vec<Vec<(u32, Tag)>> = files
        .par_iter()
        .filter_map(|path| {
            let text = strip_comments(&read_text(path).ok()?);
            let owner = owner_re.captures(&text).and_then(|c| Tag::parse(&c[1]))?;
            let c = prov_re.captures(&text)?;
            Some(
                c[1].split_whitespace()
                    .filter_map(|s| s.parse::<u32>().ok())
                    .map(|id| (id, owner))
                    .collect(),
            )
        })
        .collect();
    let mut out = FxHashMap::default();
    for part in parts {
        out.extend(part);
    }
    Ok(out)
}

pub fn load_country_parties(dir: &Path) -> Result<FxHashMap<Tag, String>> {
    let re = Regex::new(r"ruling_party\s*=\s*([a-zA-Z0-9_]+)")?;
    let files = files_with_ext(dir, "txt")?;
    let parts: Vec<Option<(Tag, String)>> = files
        .par_iter()
        .map(|path| {
            let stem = path.file_name().and_then(|s| s.to_str())?;
            let tag = stem.get(..3).and_then(|s| Tag::parse(&s.to_ascii_uppercase()))?;
            let text = strip_comments(&read_text(path).ok()?);
            let c = re.captures(&text)?;
            Some((tag, c[1].to_owned()))
        })
        .collect();
    Ok(parts.into_iter().flatten().collect())
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\\"", "\"")
}

pub fn load_localization(root: &Path, lang: &str) -> Result<FxHashMap<String, String>> {
    let dir = root.join(lang);
    let re = Regex::new(r#"^\s*([A-Za-z0-9_\.\-]+)\s*:\d*\s*"((?:[^"\\]|\\.)*)""#)?;
    let header = format!("l_{lang}:");
    let files = files_with_ext(&dir, "yml")?;
    let parts: Vec<Vec<(String, String)>> = files
        .par_iter()
        .map(|path| {
            let Ok(text) = read_text(path) else { return Vec::new() };
            let mut out = Vec::new();
            let mut in_section = false;
            for line in text.lines() {
                let line = line.trim_start_matches('\u{feff}');
                if line.contains(&header) {
                    in_section = true;
                    continue;
                }
                if !in_section || line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some(c) = re.captures(line) {
                    out.push((c[1].to_owned(), unescape(&c[2])));
                }
            }
            out
        })
        .collect();
    let mut out = FxHashMap::default();
    for part in parts {
        out.extend(part);
    }
    Ok(out)
}