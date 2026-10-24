use anyhow::{anyhow, Context, Result};
use fontdue::{Font, FontSettings};
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::path::Path;
use wgpu::util::DeviceExt;

use crate::resource::GpuTexture;

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    size: Vec2,
    bearing: Vec2,
    advance: f32,
}

/// Атлас глифов (ASCII + кириллица) в одной R8-текстуре с мипами.
pub struct FontAtlas {
    pub texture: GpuTexture,
    glyphs: FxHashMap<char, Glyph>,
    pub px: f32,
}

const ATLAS_W: usize = 2048;
const PAD: usize = 4;

impl FontAtlas {
    pub fn load(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path, px: f32) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("font {}", path.display()))?;
        let font = Font::from_bytes(bytes, FontSettings::default()).map_err(|e| anyhow!("font parse: {e}"))?;

        let mut raster = Vec::new();
        for cp in [32u32..=126, 0x0400..=0x04FF].into_iter().flatten() {
            let Some(ch) = char::from_u32(cp) else { continue };
            if font.lookup_glyph_index(ch) == 0 {
                continue;
            }
            let (m, bmp) = font.rasterize(ch, px);
            raster.push((ch, m, bmp));
        }

        let (mut x, mut y, mut row_h) = (PAD, PAD, 0usize);
        let mut pos = Vec::with_capacity(raster.len());
        for (_, m, _) in &raster {
            if x + m.width + PAD > ATLAS_W {
                x = PAD;
                y += row_h + PAD;
                row_h = 0;
            }
            pos.push((x, y));
            x += m.width + PAD;
            row_h = row_h.max(m.height);
        }
        let atlas_h = (y + row_h + PAD).next_power_of_two();
        let mut pixels = vec![0u8; ATLAS_W * atlas_h];

        let mut glyphs = FxHashMap::default();
        for ((ch, m, bmp), &(gx, gy)) in raster.iter().zip(&pos) {
            for row in 0..m.height {
                let dst = (gy + row) * ATLAS_W + gx;
                pixels[dst..dst + m.width].copy_from_slice(&bmp[row * m.width..(row + 1) * m.width]);
            }
            glyphs.insert(
                *ch,
                Glyph {
                    uv: [
                        gx as f32 / ATLAS_W as f32,
                        gy as f32 / atlas_h as f32,
                        (gx + m.width) as f32 / ATLAS_W as f32,
                        (gy + m.height) as f32 / atlas_h as f32,
                    ],
                    size: Vec2::new(m.width as f32, m.height as f32),
                    bearing: Vec2::new(m.xmin as f32, m.ymin as f32),
                    advance: m.advance_width,
                },
            );
        }

        let texture = GpuTexture::from_r8(device, queue, ATLAS_W as u32, atlas_h as u32, &pixels, true, "font atlas");
        log::info!("font atlas {}x{}, {} glyphs", ATLAS_W, atlas_h, glyphs.len());
        Ok(Self { texture, glyphs, px })
    }

    pub fn measure(&self, text: &str, scale: f32) -> f32 {
        text.chars().filter_map(|c| self.glyphs.get(&c)).map(|g| g.advance * scale).sum()
    }

    /// 6 вершин (x, y, u, v) на символ; `origin` — baseline слева.
    pub fn push_text(&self, out: &mut Vec<f32>, text: &str, origin: Vec2, scale: f32) {
        let mut x = origin.x;
        for ch in text.chars() {
            let Some(g) = self.glyphs.get(&ch) else { continue };
            if g.size.x > 0.0 {
                let x0 = x + g.bearing.x * scale;
                let y0 = origin.y + g.bearing.y * scale;
                let x1 = x0 + g.size.x * scale;
                let y1 = y0 + g.size.y * scale;
                let [u0, v0, u1, v1] = g.uv;
                out.extend_from_slice(&[
                    x0, y1, u0, v0, //
                    x0, y0, u0, v1, //
                    x1, y0, u1, v1, //
                    x0, y1, u0, v0, //
                    x1, y0, u1, v1, //
                    x1, y1, u1, v0, //
                ]);
            }
            x += g.advance * scale;
        }
    }
}

/// Статический вертексный буфер со всеми подписями.
pub struct TextBatch {
    pub buffer: wgpu::Buffer,
    pub count: u32,
}

impl TextBatch {
    pub fn new(device: &wgpu::Device, vertices: &[f32]) -> Self {
        let dummy = [0.0f32; 4];
        let data: &[f32] = if vertices.is_empty() { &dummy } else { vertices };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text batch"),
            contents: bytemuck::cast_slice(data),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self { buffer, count: (vertices.len() / 4) as u32 }
    }
}
