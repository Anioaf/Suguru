use anyhow::{Context, Result};
use rayon::prelude::*;
use std::path::Path;

pub struct GpuTexture {
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

fn downsample(src: &[u8], w: usize, h: usize, ch: usize) -> (Vec<u8>, usize, usize) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![0u8; nw * nh * ch];
    out.par_chunks_mut(nw * ch).enumerate().for_each(|(y, row)| {
        let y0 = (y * 2).min(h - 1);
        let y1 = (y * 2 + 1).min(h - 1);
        for x in 0..nw {
            let x0 = (x * 2).min(w - 1);
            let x1 = (x * 2 + 1).min(w - 1);
            for c in 0..ch {
                let s = src[(y0 * w + x0) * ch + c] as u32
                    + src[(y0 * w + x1) * ch + c] as u32
                    + src[(y1 * w + x0) * ch + c] as u32
                    + src[(y1 * w + x1) * ch + c] as u32;
                row[x * ch + c] = ((s + 2) / 4) as u8;
            }
        }
    });
    (out, nw, nh)
}

fn write_mip(queue: &wgpu::Queue, texture: &wgpu::Texture, level: u32, bytes: &[u8], w: usize, h: usize, ch: usize) {
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some((w * ch) as u32),
            rows_per_image: Some(h as u32),
        },
        wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
    );
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: usize,
    h: usize,
    data: &[u8],
    ch: usize,
    format: wgpu::TextureFormat,
    mips: bool,
    label: &str,
) -> GpuTexture {
    let levels = if mips { 32 - (w.max(h) as u32).leading_zeros() } else { 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_mip(queue, &texture, 0, data, w, h, ch);
    let mut cur: Vec<u8> = Vec::new();
    let (mut cw, mut chh) = (w, h);
    for level in 1..levels {
        let (next, nw, nh) = downsample(if level == 1 { data } else { &cur[..] }, cw, chh, ch);
        cur = next;
        cw = nw;
        chh = nh;
        write_mip(queue, &texture, level, &cur, cw, chh, ch);
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    GpuTexture { texture, view }
}

impl GpuTexture {
    pub fn from_rgb8(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, rgb: &[u8], mips: bool, label: &str,
    ) -> Self {
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        rgba.par_chunks_exact_mut(4)
            .zip(rgb.par_chunks_exact(3))
            .for_each(|(d, s)| d[..3].copy_from_slice(s));
        upload(device, queue, w as usize, h as usize, &rgba, 4, wgpu::TextureFormat::Rgba8Unorm, mips, label)
    }

    pub fn from_rgba8(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, data: &[u8], mips: bool, label: &str,
    ) -> Self {
        upload(device, queue, w as usize, h as usize, data, 4, wgpu::TextureFormat::Rgba8Unorm, mips, label)
    }

    pub fn from_r8(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, data: &[u8], mips: bool, label: &str,
    ) -> Self {
        upload(device, queue, w as usize, h as usize, data, 1, wgpu::TextureFormat::R8Unorm, mips, label)
    }

    pub fn from_r16_uint(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, data: &[u16], label: &str,
    ) -> Self {
        upload(
            device, queue, w as usize, h as usize,
            bytemuck::cast_slice(data), 2, wgpu::TextureFormat::R16Uint, false, label,
        )
    }

    pub fn write_rgba8(&self, queue: &wgpu::Queue, w: u32, h: u32, data: &[u8]) {
        write_mip(queue, &self.texture, 0, data, w as usize, h as usize, 4);
    }

    pub fn from_file(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path, mips: bool) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("texture {}", path.display()))?
            .flipv()
            .to_rgb8();
        let (w, h) = img.dimensions();
        Ok(Self::from_rgb8(device, queue, w, h, img.as_raw(), mips, &path.to_string_lossy()))
    }

    pub fn from_file_gray(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path, mips: bool) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("texture {}", path.display()))?
            .flipv()
            .to_luma8();
        let (w, h) = img.dimensions();
        Ok(Self::from_r8(device, queue, w, h, img.as_raw(), mips, &path.to_string_lossy()))
    }
}