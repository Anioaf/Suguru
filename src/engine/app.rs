use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use std::{sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::KeyCode,
    window::{Fullscreen, Window, WindowId},
};

use super::{Config, Input};
use crate::game::{World, WORLD_HEIGHT, WORLD_WIDTH};
use crate::render::{Camera, FrameParams, Gfx, Renderer};

/// GPU-часть приложения. Порядок полей = порядок drop: renderer → gfx → window.
struct Gpu {
    renderer: Renderer,
    gfx: Gfx,
    window: Arc<Window>,
    start: Instant,
    last: f64,
}

struct App {
    cfg: Config,
    world: World,
    camera: Camera,
    input: Input,
    gpu: Option<Gpu>,
    error: Option<anyhow::Error>,
}

pub fn run(cfg: Config) -> Result<()> {
    // Данные грузим ДО создания окна: ошибки контента видны сразу, а парсинг идет параллельно.
    log::info!("content root: {}", cfg.content_root.display());
    let world = World::load(&cfg)?;
    let mut app = App {
        cfg,
        world,
        camera: Camera::new(Vec2::new(WORLD_WIDTH, WORLD_HEIGHT)),
        input: Input::default(),
        gpu: None,
        error: None,
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
        let gfx = Gfx::new(window.clone())?;
        let renderer = Renderer::new(&gfx, &self.cfg, &self.world)?;
        Ok(Gpu { renderer, gfx, window, start: Instant::now(), last: 0.0 })
    }

    fn redraw(&mut self, el: &ActiveEventLoop) {
        let Some(gpu) = self.gpu.as_mut() else { return };

        let now = gpu.start.elapsed().as_secs_f64();
        let dt = ((now - gpu.last) as f32).min(0.1);
        gpu.last = now;

        if self.input.just_pressed(KeyCode::Escape) {
            el.exit();
            return;
        }
        gpu.renderer.poll_hot_reload(&gpu.gfx.device, now, self.input.just_pressed(KeyCode::F10));
        self.camera.update(dt, &self.input);

        let size = gpu.window.inner_size();
        if size.width == 0 || size.height == 0 {
            self.input.end_frame();
            return; // свернуто
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

        let Some(frame) = gpu.gfx.acquire() else { return };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sel = &self.world.selection;
        gpu.renderer.render(
            &gpu.gfx.device,
            &gpu.gfx.queue,
            &view,
            &FrameParams {
                camera: &self.camera,
                size: (size.width, size.height),
                time: now as f32,
                selected_rgb: sel.province.map(|_| -> Vec3 { sel.rgb }),
            },
        );
        frame.present();
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

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }
}
