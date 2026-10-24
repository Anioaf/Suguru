use anyhow::{Context, Result};
use image::{imageops, imageops::FilterType, GrayImage, ImageBuffer, Pixel, RgbaImage};
use std::path::Path;
use wgpu::util::DeviceExt;

pub struct GpuTexture {
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// wgpu не умеет генерировать мипы сам — считаем на CPU (Triangle-фильтр).
fn mip_chain<P>(base: ImageBuffer<P, Vec<u8>>) -> Vec<ImageBuffer<P, Vec<u8>>>
where
    P: Pixel<Subpixel = u8> + 'static,
{
    let mut out = vec![base];
    loop {
        let last = out.last().unwrap();
        let (w, h) = last.dimensions();
        if w == 1 && h == 1 {
            break;
        }
        let next = imageops::resize(last, (w / 2).max(1), (h / 2).max(1), FilterType::Triangle);
        out.push(next);
    }
    out
}

fn upload<P>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: ImageBuffer<P, Vec<u8>>,
    format: wgpu::TextureFormat,
    mips: bool,
    label: &str,
) -> GpuTexture
where
    P: Pixel<Subpixel = u8> + 'static,
{
    let (width, height) = img.dimensions();
    let levels = if mips { mip_chain(img) } else { vec![img] };
    let mut data = Vec::new();
    for l in &levels {
        data.extend_from_slice(l.as_raw());
    }
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    GpuTexture { texture, view }
}

impl GpuTexture {
    /// RGB → RGBA8 (RGB8 в wgpu не существует).
    pub fn from_rgb8(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, rgb: &[u8], mips: bool, label: &str,
    ) -> Self {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for p in rgb.chunks_exact(3) {
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        let img = RgbaImage::from_raw(w, h, rgba).expect("rgb size mismatch");
        upload(device, queue, img, wgpu::TextureFormat::Rgba8Unorm, mips, label)
    }

    pub fn from_r8(
        device: &wgpu::Device, queue: &wgpu::Queue,
        w: u32, h: u32, data: &[u8], mips: bool, label: &str,
    ) -> Self {
        let img = GrayImage::from_raw(w, h, data.to_vec()).expect("r8 size mismatch");
        upload(device, queue, img, wgpu::TextureFormat::R8Unorm, mips, label)
    }

    /// Файл переворачивается по вертикали: строка 0 = низ карты.
    pub fn from_file(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path, mips: bool) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("texture {}", path.display()))?
            .flipv()
            .to_rgb8();
        let (w, h) = img.dimensions();
        Ok(Self::from_rgb8(device, queue, w, h, img.as_raw(), mips, &path.to_string_lossy()))
    }
}
