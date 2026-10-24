use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

use super::{Camera, FontAtlas, Gfx, TextBatch};
use crate::engine::Config;
use crate::game::{World, WORLD_HEIGHT, WORLD_WIDTH};
use crate::resource::{GpuTexture, ShaderFile};

const COMMON_WGSL: &str = include_str!("../../content/gfx/FX/common.wgsl");
const MAP_WGSL: &str = include_str!("../../content/gfx/FX/map.wgsl");
const TEXT_WGSL: &str = include_str!("../../content/gfx/FX/text.wgsl");

/// Все, что нужно для одного кадра (аналог render view в Source 2).
pub struct FrameParams<'a> {
    pub camera: &'a Camera,
    pub size: (u32, u32),
    pub time: f32,
    /// None — ничего не выбрано
    pub selected_rgb: Option<Vec3>,
}

/// Должен совпадать с `FrameUniform` в common.wgsl.
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
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState { cull_mode: cull, ..Default::default() },
        depth_stencil: None, // всё лежит в плоскости z=0 — depth-буфер не нужен
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Компилирует шейдер и строит пайплайн. Ошибки валидации перехватываются error-scope'ом:
/// битый шейдер не роняет движок, остается предыдущий пайплайн.
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

impl Renderer {
    pub fn new(gfx: &Gfx, cfg: &Config, world: &World) -> Result<Self> {
        let (device, queue) = (&gfx.device, &gfx.queue);
        let format = gfx.config.format;

        // ── ресурсы ──
        let m = &world.map;
        let tex_norm = GpuTexture::from_file(device, queue, &cfg.path("map/normal.bmp"), true)?;
        let tex_height = GpuTexture::from_file(device, queue, &cfg.path("map/heightmap.bmp"), true)?;
        let tex_prov = GpuTexture::from_rgb8(device, queue, m.width, m.height, &m.rgb, false, "provinces");
        let state_img = m.owner_color_image(&world.country_colors);
        let tex_state = GpuTexture::from_rgb8(device, queue, m.width, m.height, &state_img, false, "states");

        let font_path = cfg.find_font().context("не найден ни один шрифт (см. Config::font_candidates)")?;
        let font = FontAtlas::load(device, queue, &font_path, cfg.font_px)?;

        let mut verts = Vec::new();
        for l in &world.labels {
            let w = font.measure(&l.name, l.size);
            let origin = glam::Vec2::new(l.pos.x - w * 0.5, l.pos.y - 0.3 * font.px * l.size);
            font.push_text(&mut verts, &l.name, origin, l.size);
        }
        let labels = TextBatch::new(device, &verts);

        // ── сэмплеры ──
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
        let smp_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let smp_text = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // ── uniform + bind groups ──
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
            entries: &[tex_entry(0), tex_entry(1), tex_entry(2), tex_entry(3), smp_entry(4), smp_entry(5)],
        });
        let map_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map bg"),
            layout: &map_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tex_norm.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tex_height.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&tex_prov.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tex_state.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&smp_linear) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&smp_nearest) },
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

        // ── геометрия ──
        #[rustfmt::skip]
        let verts: [f32; 20] = [
            0.0,         0.0,          0.0, 0.0, 0.0,
            WORLD_WIDTH, 0.0,          0.0, 1.0, 0.0,
            WORLD_WIDTH, WORLD_HEIGHT, 0.0, 1.0, 1.0,
            0.0,         WORLD_HEIGHT, 0.0, 0.0, 1.0,
        ];
        let indices: [u32; 6] = [0, 1, 2, 0, 2, 3];
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // ── шейдеры ──
        let fx = |n: &str| cfg.path(&format!("gfx/FX/{n}"));
        let mut map_file = ShaderFile::new(fx("map.wgsl"), fx("common.wgsl"), MAP_WGSL, COMMON_WGSL);
        let mut text_file = ShaderFile::new(fx("text.wgsl"), fx("common.wgsl"), TEXT_WGSL, COMMON_WGSL);

        let map_pipeline = try_build(device, "map", &mut map_file, |m| {
            make_pipeline(device, "map", &map_layout, m, format, 20, &MAP_ATTRS, Some(wgpu::Face::Back))
        })
        .context("map.wgsl не скомпилировался (см. лог выше)")?;
        let text_pipeline = try_build(device, "text", &mut text_file, |m| {
            make_pipeline(device, "text", &text_layout, m, format, 16, &TEXT_ATTRS, None)
        })
        .context("text.wgsl не скомпилировался (см. лог выше)")?;

        Ok(Self {
            format, map_file, text_file, map_layout, text_layout, map_pipeline, text_pipeline,
            frame_buf, frame_bg, map_bg, text_bg, quad_vb, quad_ib, labels, last_poll: 0.0,
        })
    }

    /// Автоперезагрузка WGSL по mtime (раз в 250 мс) или принудительно по F10.
    pub fn poll_hot_reload(&mut self, device: &wgpu::Device, now: f64, force: bool) {
        if !force && now - self.last_poll < 0.25 {
            return;
        }
        self.last_poll = now;
        let format = self.format;

        if force || self.map_file.changed() {
            let layout = &self.map_layout;
            if let Some(p) = try_build(device, "map", &mut self.map_file, |m| {
                make_pipeline(device, "map", layout, m, format, 20, &MAP_ATTRS, Some(wgpu::Face::Back))
            }) {
                self.map_pipeline = p;
            }
        }
        if force || self.text_file.changed() {
            let layout = &self.text_layout;
            if let Some(p) = try_build(device, "text", &mut self.text_file, |m| {
                make_pipeline(device, "text", layout, m, format, 16, &TEXT_ATTRS, None)
            }) {
                self.text_pipeline = p;
            }
        }
    }

    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::TextureView, p: &FrameParams) {
        let aspect = p.size.0 as f32 / p.size.1 as f32;
        let vp = p.camera.proj(aspect) * p.camera.view();
        let sel = p.selected_rgb.map_or([0.0; 4], |c| [c.x, c.y, c.z, 1.0]);
        let u = FrameUniform {
            view_proj: vp.to_cols_array_2d(),
            selected: sel,
            params: [p.time, WORLD_WIDTH, 0.0, 0.0],
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

            // карта: 3 копии мира одним instanced draw-call'ом
            pass.set_pipeline(&self.map_pipeline);
            pass.set_bind_group(1, &self.map_bg, &[]);
            pass.set_vertex_buffer(0, self.quad_vb.slice(..));
            pass.set_index_buffer(self.quad_ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..6, 0, 0..3);

            // подписи: тоже 3 инстанса
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
