use rustc_hash::FxHashSet;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

#[derive(Default)]
pub struct Input {
    down: FxHashSet<KeyCode>,
    pressed: FxHashSet<KeyCode>,
    click: bool,
    scroll: f32,
    cursor: (f64, f64),
}

impl Input {
    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.click = false;
        self.scroll = 0.0;
    }

    pub fn handle_event(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(code), state, repeat: false, .. },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.down.insert(*code);
                    self.pressed.insert(*code);
                }
                ElementState::Released => {
                    self.down.remove(code);
                }
            },
            WindowEvent::Focused(false) => self.down.clear(),
            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::LineDelta(_, y) => self.scroll += *y,
                MouseScrollDelta::PixelDelta(p) => self.scroll += (p.y / 40.0) as f32,
            },
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                self.click = true
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
            _ => {}
        }
    }

    pub fn is_down(&self, k: KeyCode) -> bool { self.down.contains(&k) }
    pub fn just_pressed(&self, k: KeyCode) -> bool { self.pressed.contains(&k) }
    pub fn clicked(&self) -> bool { self.click }
    pub fn scroll(&self) -> f32 { self.scroll }
    pub fn cursor(&self) -> (f64, f64) { self.cursor }
}
