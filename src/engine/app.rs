use anyhow::{Context, Result};
use glam::Vec2;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::KeyCode,
    window::{Fullscreen, Window, WindowId},
};

use super::{Config, Input};
use crate::game::{Command, World};
use crate::render::{Camera, CameraInput, FrameParams, Gfx, LabelRef, Renderer, SceneDesc, PALETTE_SIDE};

const SPEED_KEYS: [KeyCode; 5] =
    [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];

#[derive(Default)]
struct Stats {
    frames: u32,
    since: f32,
    dt_max: f32,
    cpu: f32,
    acquire: f32,
    render: f32,
}

impl Stats {
    fn push(&mut self, dt: f32, cpu: f32, acquire: f32, render: f32) -> Option<String> {
        self.frames += 1;
        self.since += dt;
        self.dt_max = self.dt_max.max(dt);
        self.cpu += cpu;
        self.acquire += acquire;
        self.render += render;
        if self.since < 1.0 {
            return None;
        }
        let n = self.frames as f32;
        let s = format!(
            "Suguru | fps {:.0} | avg {:.2} max {:.2} мс | cpu {:.2} acq {:.2} render {:.2}",
            n / self.since,
            self.since / n * 1000.0,
            self.dt_max * 1000.0,
            self.cpu / n * 1000.0,
            self.acquire / n * 1000.0,
            self.render / n * 1000.0,
        );
        *self = Self::default();
        Some(s)
    }
}

struct Gpu {
    renderer: Renderer,
    gfx: Gfx,
    window: Arc<Window>,
    start: Instant,
    last: f64,
    stats: Stats,
}

struct App {
    cfg: Config,
    world: World,
    camera: Camera,
    input: Input,
    gpu: Option<Gpu>,
    error: Option<anyhow::Error>,
    next_frame: Instant,
}

fn axis(input: &Input, pos: KeyCode, neg: KeyCode) -> f32 {
    (input.is_down(pos) as i8 - input.is_down(neg) as i8) as f32
}

fn camera_input(input: &Input) -> CameraInput {
    CameraInput {
        pan: Vec2::new(
            axis(input, KeyCode::ArrowRight, KeyCode::ArrowLeft),
            axis(input, KeyCode::ArrowUp, KeyCode::ArrowDown),
        ),
        scroll: input.scroll(),
    }
}

pub fn run(cfg: Config) -> Result<()> {
    log::info!("content root: {}", cfg.content_root.display());
    let world = World::load(&cfg.content_root, &cfg.language)?;
    let camera = Camera::new(world.size());
    let mut app = App {
        cfg,
        world,
        camera,
        input: Input::default(),
        gpu: None,
        error: None,
        next_frame: Instant::now(),
    };

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;

    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

impl App {
    fn init_gpu(&self, el: &ActiveEventLoop) -> Result<Gpu> {
        let mut attrs = Window::default_attributes()
            .with_title(self.cfg.title.clone())
            .with_inner_size(LogicalSize::new(self.cfg.width, self.cfg.height));
        if self.cfg.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(el.create_window(attrs).context("не удалось создать окно")?);
        let gfx = Gfx::new(window.clone(), self.cfg.vsync)?;

        let font_path = self
            .cfg
            .find_font()
            .context("не найден ни один шрифт (см. Config::font_candidates)")?;
        let map = &self.world.map;
        let scene = SceneDesc {
            content_root: &self.cfg.content_root,
            font_path: &font_path,
            font_px: self.cfg.font_px,
            world_size: self.world.size(),
            map_size: (map.width, map.height),
            province_ids: &map.province_ids,
            palette: map.owner_palette(&self.world.country_colors, PALETTE_SIDE),
            labels: self
                .world
                .labels
                .iter()
                .map(|l| LabelRef { text: &l.name, pos: l.pos, size: l.size })
                .collect(),
        };
        let renderer = Renderer::new(&gfx, &scene)?;
        Ok(Gpu {
            renderer,
            gfx,
            window,
            start: Instant::now(),
            last: 0.0,
            stats: Stats::default(),
        })
    }

    fn redraw(&mut self, el: &ActiveEventLoop) {
        let Some(gpu) = self.gpu.as_mut() else { return };

        let frame_start = Instant::now();
        let now = gpu.start.elapsed().as_secs_f64();
        let raw_dt = (now - gpu.last) as f32;
        let dt = raw_dt.min(0.1);
        gpu.last = now;

        if self.input.just_pressed(KeyCode::Escape) {
            el.exit();
            return;
        }
        gpu.renderer.poll_hot_reload(&gpu.gfx.device, now, self.input.just_pressed(KeyCode::F10));
        self.camera.update(dt, camera_input(&self.input));

        if self.input.just_pressed(KeyCode::Space) {
            self.world.submit(Command::TogglePause);
        }
        for (i, key) in SPEED_KEYS.iter().enumerate() {
            if self.input.just_pressed(*key) {
                self.world.submit(Command::SetSpeed(i as u8 + 1));
            }
        }
        self.world.advance(Duration::from_secs_f32(dt));

        let size = gpu.window.inner_size();
        if size.width == 0 || size.height == 0 {
            self.input.end_frame();
            return;
        }

        if self.input.clicked() {
            let (mx, my) = self.input.cursor();
            let nx = 2.0 * (mx as f32 / size.width as f32) - 1.0;
            let ny = 1.0 - 2.0 * (my as f32 / size.height as f32);
            let aspect = size.width as f32 / size.height as f32;
            if let Some(p) = self.camera.screen_to_world(Vec2::new(nx, ny), aspect) {
                self.world.pick(p);
            }
        }
        self.input.end_frame();

        let t0 = Instant::now();
        let frame = match gpu.gfx.acquire() {
            Ok(Some(f)) => f,
            Ok(None) => return,
            Err(e) => {
                self.error = Some(e);
                el.exit();
                return;
            }
        };
        let t1 = Instant::now();
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        gpu.renderer.render(
            &gpu.gfx.device,
            &gpu.gfx.queue,
            &view,
            &FrameParams {
                camera: &self.camera,
                size: (size.width, size.height),
                time: now as f32,
                selected: self.world.selection.province,
            },
        );
        frame.present();
        let t2 = Instant::now();

        if let Some(s) = gpu.stats.push(
            raw_dt,
            (t0 - frame_start).as_secs_f32(),
            (t1 - t0).as_secs_f32(),
            (t2 - t1).as_secs_f32(),
        ) {
            gpu.window.set_title(&s);
            log::info!("{s}");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        match self.init_gpu(el) {
            Ok(g) => self.gpu = Some(g),
            Err(e) => {
                self.error = Some(e);
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        self.input.handle_event(&ev);
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.gfx.resize(s.width, s.height);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(el),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(g) = &self.gpu else { return };
        let s = g.window.inner_size();
        if s.width == 0 || s.height == 0 || g.window.is_minimized().unwrap_or(false) {
            el.set_control_flow(ControlFlow::Wait);
            return;
        }
        match self.cfg.fps_cap {
            Some(cap) if cap > 0 => {
                let step = Duration::from_secs_f64(1.0 / cap as f64);
                let now = Instant::now();
                if now >= self.next_frame {
                    self.next_frame = (self.next_frame + step).max(now);
                    g.window.request_redraw();
                }
                el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
            }
            _ => {
                el.set_control_flow(ControlFlow::Poll);
                g.window.request_redraw();
            }
        }
    }
}