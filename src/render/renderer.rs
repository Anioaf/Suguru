use anyhow::{ensure, Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::path::Path;
use wgpu::util::DeviceExt;

use super::{Camera, FontAtlas, Gfx, TextBatch};
use crate::resource::{GpuTexture, ShaderFile};

const COMMON_WGSL: &str = include_str!("../../content/gfx/FX/common.wgsl");
const MAP_WGSL: &str = include_str!("../../content/gfx/FX/map.wgsl");
const TEXT_WGSL: &str = include_str!("../../content/gfx/FX/text.wgsl");

pub const PALETTE_SIDE: u32 = 256;

pub struct LabelRef<'a> {
    pub text: &'a str,
    pub pos: Vec2,
    pub size: f32,
}

pub struct SceneDesc<'a> {
    pub content_root: &'a Path,
    pub font_path: &'a Path,
    pub font_px: f32,
    pub world_size: Vec2,
    pub map_size: (u32, u32),
    pub province_ids: &'a [u16],
    pub palette: Vec<u8>,
    pub labels: Vec<LabelRef<'a>>,
}

pub struct FrameParams<'a> {
    pub camera: &'a Camera,
    pub size: (u32, u32),
    pub time: f32,
    pub selected: Option<u32>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    selected: [f32; 4],
    params: [f32; 4],
}

const MAP_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
const TEXT_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x4];

pub struct Renderer {
    format: wgpu::TextureFormat,
    world_size: Vec2,
    map_file: ShaderFile,
    text_file: ShaderFile,
    map_layout: wgpu::PipelineLayout,
    text_layout: wgpu::PipelineLayout,
    map_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    frame_buf: wgpu::Buffer,
    frame_bg: wgpu::BindGroup,
    map_bg: wgpu::BindGroup,
    text_bg: wgpu::BindGroup,
    quad_vb: wgpu::Buffer,
    quad_ib: wgpu::Buffer,
    labels: TextBatch,
    palette: GpuTexture,
    last_poll: f64,
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uint_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn smp_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    stride: u64,
    attrs: &[wgpu::VertexAttribute],
    cull: Option<wgpu::Face>,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: "vs_main",
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: attrs,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: "fs_main",
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState { cull_mode: cull, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn try_build(
    device: &wgpu::Device,
    label: &str,
    file: &mut ShaderFile,
    build: impl FnOnce(&wgpu::ShaderModule) -> wgpu::RenderPipeline,
) -> Option<wgpu::RenderPipeline> {
    let code = file.read();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(code.into()),
    });
    let pipeline = build(&module);
    match pollster::block_on(device.pop_error_scope()) {
        Some(err) => {
            log::error!("shader '{label}' failed:\n{err}");
            None
        }
        None => {
            log::info!("shader '{label}' (re)loaded");
            Some(pipeline)
        }
    }
}

fn build_map(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    m: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    make_pipeline(device, "map", layout, m, format, 20, &MAP_ATTRS, Some(wgpu::Face::Back), None)
}

fn build_text(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    m: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    make_pipeline(device, "text", layout, m, format, 16, &TEXT_ATTRS, None, Some(wgpu::BlendState::ALPHA_BLENDING))
}

impl Renderer {
    pub fn new(gfx: &Gfx, scene: &SceneDesc) -> Result<Self> {
        let (device, queue) = (&gfx.device, &gfx.queue);
        let format = gfx.config.format;
        let root = scene.content_root;
        let (map_w, map_h) = scene.map_size;

        ensure!(
            scene.province_ids.len() == map_w as usize * map_h as usize,
            "province_ids: {} элементов, ожидалось {}x{}",
            scene.province_ids.len(),
            map_w,
            map_h
        );
        ensure!(
            scene.palette.len() == (PALETTE_SIDE * PALETTE_SIDE * 4) as usize,
            "palette: {} байт, ожидалось {}",
            scene.palette.len(),
            PALETTE_SIDE * PALETTE_SIDE * 4
        );

        let tex_norm = GpuTexture::from_file(device, queue, &root.join("map/normal.bmp"), true)?;
        let tex_height = GpuTexture::from_file_gray(device, queue, &root.join("map/heightmap.bmp"), true)?;
        let tex_prov = GpuTexture::from_r16_uint(device, queue, map_w, map_h, scene.province_ids, "provinces");
        let palette = GpuTexture::from_rgba8(device, queue, PALETTE_SIDE, PALETTE_SIDE, &scene.palette, false, "palette");

        let font = FontAtlas::load(device, queue, scene.font_path, scene.font_px)?;

        let mut verts = Vec::new();
        for l in &scene.labels {
            let w = font.measure(l.text, l.size);
            let origin = Vec2::new(l.pos.x - w * 0.5, l.pos.y - 0.3 * font.px * l.size);
            font.push_text(&mut verts, l.text, origin, l.size);
        }
        let labels = TextBatch::new(device, &verts);

        let smp_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        let smp_text = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let frame_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bg"),
            layout: &frame_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() }],
        });

        let map_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map bgl"),
            entries: &[tex_entry(0), tex_entry(1), uint_entry(2), tex_entry(3), smp_entry(4)],
        });
        let map_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map bg"),
            layout: &map_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tex_norm.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tex_height.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&tex_prov.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&palette.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&smp_linear) },
            ],
        });

        let text_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text bgl"),
            entries: &[tex_entry(0), smp_entry(1)],
        });
        let text_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text bg"),
            layout: &text_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&font.texture.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&smp_text) },
            ],
        });

        let map_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map layout"),
            bind_group_layouts: &[&frame_bgl, &map_bgl],
            push_constant_ranges: &[],
        });
        let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text layout"),
            bind_group_layouts: &[&frame_bgl, &text_bgl],
            push_constant_ranges: &[],
        });

        let (ww, wh) = (scene.world_size.x, scene.world_size.y);
        #[rustfmt::skip]
        let quad: [f32; 20] = [
            0.0, 0.0, 0.0, 0.0, 0.0,
            ww,  0.0, 0.0, 1.0, 0.0,
            ww,  wh,  0.0, 1.0, 1.0,
            0.0, wh,  0.0, 0.0, 1.0,
        ];
        let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad vb"),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let fx = |n: &str| root.join("gfx/FX").join(n);
        let mut map_file = ShaderFile::new(fx("map.wgsl"), fx("common.wgsl"), MAP_WGSL, COMMON_WGSL);
        let mut text_file = ShaderFile::new(fx("text.wgsl"), fx("common.wgsl"), TEXT_WGSL, COMMON_WGSL);

        let map_pipeline = try_build(device, "map", &mut map_file, |m| build_map(device, &map_layout, m, format))
            .context("map.wgsl не скомпилировался (см. лог выше)")?;
        let text_pipeline = try_build(device, "text", &mut text_file, |m| build_text(device, &text_layout, m, format))
            .context("text.wgsl не скомпилировался (см. лог выше)")?;

        Ok(Self {
            format,
            world_size: scene.world_size,
            map_file,
            text_file,
            map_layout,
            text_layout,
            map_pipeline,
            text_pipeline,
            frame_buf,
            frame_bg,
            map_bg,
            text_bg,
            quad_vb,
            quad_ib,
            labels,
            palette,
            last_poll: 0.0,
        })
    }

    #[allow(dead_code)]
    pub fn update_palette(&self, queue: &wgpu::Queue, rgba: &[u8]) {
        self.palette.write_rgba8(queue, PALETTE_SIDE, PALETTE_SIDE, rgba);
    }

    pub fn poll_hot_reload(&mut self, device: &wgpu::Device, now: f64, force: bool) {
        if !force && now - self.last_poll < 0.25 {
            return;
        }
        self.last_poll = now;
        let format = self.format;

        if force || self.map_file.changed() {
            let layout = &self.map_layout;
            if let Some(p) = try_build(device, "map", &mut self.map_file, |m| build_map(device, layout, m, format)) {
                self.map_pipeline = p;
            }
        }
        if force || self.text_file.changed() {
            let layout = &self.text_layout;
            if let Some(p) = try_build(device, "text", &mut self.text_file, |m| build_text(device, layout, m, format)) {
                self.text_pipeline = p;
            }
        }
    }

    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::TextureView, p: &FrameParams) {
        let aspect = p.size.0 as f32 / p.size.1 as f32;
        let vp = p.camera.proj(aspect) * p.camera.view();
        let sel = p.selected.map_or([0.0; 4], |id| [id as f32, 0.0, 0.0, 1.0]);
        let u = FrameUniform {
            view_proj: vp.to_cols_array_2d(),
            selected: sel,
            params: [p.time, self.world_size.x, 0.0, 0.0],
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&u));

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.12, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_bind_group(0, &self.frame_bg, &[]);

            pass.set_pipeline(&self.map_pipeline);
            pass.set_bind_group(1, &self.map_bg, &[]);
            pass.set_vertex_buffer(0, self.quad_vb.slice(..));
            pass.set_index_buffer(self.quad_ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..6, 0, 0..3);

            if self.labels.count > 0 {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(1, &self.text_bg, &[]);
                pass.set_vertex_buffer(0, self.labels.buffer.slice(..));
                pass.draw(0..self.labels.count, 0..3);
            }
        }
        queue.submit(std::iter::once(enc.finish()));
    }
}