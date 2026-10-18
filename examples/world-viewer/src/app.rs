use crate::{
    Options,
    assets::Assets,
    camera::Camera,
    renderer::{self, Renderer},
    scene::Scene,
};
use anyhow::{Context, Result};
use std::{collections::HashSet, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct WindowState {
    window: Arc<Window>,
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}
struct App {
    scene: Scene,
    assets: Assets,
    camera: Camera,
    options: Options,
    state: Option<WindowState>,
    keys: HashSet<KeyCode>,
    captured: bool,
    last_frame: Instant,
    last_title: Instant,
    error: Option<anyhow::Error>,
}
impl App {
    fn capture(&mut self, capture: bool) {
        let Some(state) = &self.state else {
            return;
        };
        self.captured = capture
            && state
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| state.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
        if !self.captured {
            let _ = state.window.set_cursor_grab(CursorGrabMode::None);
        }
        state.window.set_cursor_visible(!self.captured);
        self.keys.clear();
    }
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("ZenKit world viewer | click to fly | Esc releases mouse")
                    .with_inner_size(winit::dpi::PhysicalSize::new(
                        self.options.width,
                        self.options.height,
                    )),
            )?,
        );
        let instance = renderer::instance();
        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let (renderer, format, adapter) = pollster::block_on(Renderer::new(
            &instance,
            Some(&surface),
            &self.scene,
            &self.assets,
            size.width.max(1),
            size.height.max(1),
        ))?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("no surface configuration")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&renderer.device, &config);
        self.state = Some(WindowState {
            window,
            renderer,
            surface,
            config,
        });
        self.last_frame = Instant::now();
        eprintln!(
            "Click: capture mouse | WASD: fly | Q/E: down/up | Shift: faster | wheel: speed | Home: reset | P: print position | Esc: release, then exit"
        );
        Ok(())
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && let Err(e) = self.initialize(event_loop)
        {
            self.error = Some(e);
            event_loop.exit();
        }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if self.captured
            && let DeviceEvent::MouseMotion { delta } = event
        {
            self.camera.look(delta.0, delta.1);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => self.capture(false),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.capture(true),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if event.state == ElementState::Pressed && !event.repeat {
                        match key {
                            KeyCode::Escape => {
                                if self.captured {
                                    self.capture(false);
                                } else {
                                    event_loop.exit();
                                }
                            }
                            KeyCode::Home => self.camera.reset(),
                            KeyCode::KeyP => eprintln!(
                                "--position {:.1} {:.1} {:.1} --yaw {:.2} --pitch {:.2}",
                                self.camera.position.x * 100.0,
                                self.camera.position.y * 100.0,
                                -self.camera.position.z * 100.0,
                                self.camera.yaw.to_degrees(),
                                self.camera.pitch.to_degrees()
                            ),
                            _ => {}
                        }
                    }
                    if self.captured && event.state == ElementState::Pressed {
                        self.keys.insert(key);
                    } else {
                        self.keys.remove(&key);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.camera.speed = (self.camera.speed * 1.2_f32.powf(amount)).clamp(0.1, 2000.0);
            }
            WindowEvent::Resized(size) => {
                if size.width > 0
                    && size.height > 0
                    && let Some(state) = &mut self.state
                {
                    state.config.width = size.width;
                    state.config.height = size.height;
                    state
                        .surface
                        .configure(&state.renderer.device, &state.config);
                    state.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                self.camera
                    .step(&self.keys, (now - self.last_frame).as_secs_f32());
                self.last_frame = now;
                let Some(state) = &mut self.state else {
                    return;
                };
                if state.window.inner_size().width == 0 || state.window.inner_size().height == 0 {
                    return;
                }
                let frame = match state.surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state
                            .surface
                            .configure(&state.renderer.device, &state.config);
                        return;
                    }
                    Err(wgpu::SurfaceError::Timeout) => return,
                    Err(e) => {
                        self.error = Some(e.into());
                        event_loop.exit();
                        return;
                    }
                };
                let encoder = state.renderer.draw(
                    &frame.texture.create_view(&Default::default()),
                    &self.camera,
                    state.config.width,
                    state.config.height,
                );
                state.renderer.queue.submit([encoder.finish()]);
                state.window.pre_present_notify();
                frame.present();
                if now.duration_since(self.last_title).as_secs_f32() > 0.5 {
                    state.window.set_title(&format!(
                        "ZenKit | {} | {} VOBs | {:.1} m/s | WASD Q/E Shift | Esc releases mouse",
                        self.options.world, self.scene.vobs, self.camera.speed
                    ));
                    self.last_title = now;
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}
pub fn run(scene: Scene, assets: Assets, camera: Camera, options: Options) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene,
        assets,
        camera,
        options,
        state: None,
        keys: HashSet::new(),
        captured: false,
        last_frame: Instant::now(),
        last_title: Instant::now(),
        error: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}
