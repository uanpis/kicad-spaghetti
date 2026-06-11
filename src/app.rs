use crate::draw::*;
use crate::gui::EguiRenderer;
use crate::sim::Sim;
use egui_wgpu::{ScreenDescriptor, wgpu};
use std::sync::Arc;
use std::time::Instant;
use wgpu::CurrentSurfaceTexture;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

pub struct AppState {
    pub sim: Sim,
    pub draw2d: Draw2D,
    pub leftpanel: bool,

    pub paused: bool,
    pub fps: f32,
    pub time: Instant,
    pub iterations: u64,

    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
}

impl AppState {
    async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Self {
        let power_pref = wgpu::PowerPreference::default();
        // TODO cache result to improve startup time
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power_pref,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let features = wgpu::Features::empty();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: features,
                required_limits: Default::default(),
                experimental_features: Default::default(),
                memory_hints: Default::default(),
                trace: Default::default(),
            })
            .await
            .expect("Failed to create device");

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let selected_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|d| **d == selected_format)
            .expect("failed to select proper surface texture format!");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: *swapchain_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 0,
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &surface_config);

        let scale_factor = 1.0;

        let sim = Sim::new();
        let draw2d = Draw2D::new(&device, &queue, selected_format, &sim.snapshot);

        let time = Instant::now();

        Self {
            sim,
            draw2d,

            leftpanel: true,
            paused: false,
            fps: 0.0,
            time,
            iterations: 0,

            device,
            queue,
            surface,
            surface_config,
            scale_factor,
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        let z: f32 = 2.0 / self.sim.snapshot.radius;
        self.draw2d.update_globals(
            &self.queue,
            GpuGlobals {
                zoom: [z, z],
                pan: [-self.sim.snapshot.center.x, -self.sim.snapshot.center.y],
                aspect_ratio: self.surface_config.width as f32 / self.surface_config.height as f32,
                _pad: 0.0,
            },
        )
    }

    fn update_geom(&mut self) {
        let snapshot = self.sim.get_snapshot();
        if snapshot.new {
            snapshot.new = false;

            self.draw2d.rebuild(&self.device, &self.queue, snapshot);
            let z: f32 = 2.0 / snapshot.radius;
            self.draw2d.update_globals(
                &self.queue,
                GpuGlobals {
                    zoom: [z, z],
                    pan: [-snapshot.center.x, -snapshot.center.y],
                    aspect_ratio: self.surface_config.width as f32
                        / self.surface_config.height as f32,
                    _pad: 0.0,
                },
            )
        }
    }

    pub fn init(&mut self) {
        self.sim.import();
    }
}

pub struct App {
    instance: wgpu::Instance,
    state: Option<AppState>,
    window: Option<Arc<Window>>,
    egui_renderer: Option<EguiRenderer>,
}

impl App {
    pub fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        Self {
            instance,
            state: None,
            window: None,

            egui_renderer: None,
        }
    }

    async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);
        let initial_width = 1360;
        let initial_height = 768;

        let _ = window.request_inner_size(PhysicalSize::new(initial_width, initial_height));

        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("Failed to create surface!");

        let state = AppState::new(&self.instance, surface, initial_width, initial_width).await;
        let egui_renderer = EguiRenderer::new(&state.device, state.surface_config.format, &window);

        self.window.get_or_insert(window);
        self.state.get_or_insert(state);
        self.egui_renderer.get_or_insert(egui_renderer);
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.state.as_mut().unwrap().resize_surface(width, height);
        }
    }

    fn handle_redraw(&mut self) {
        let state = self.state.as_mut().unwrap();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: self.window.as_ref().unwrap().scale_factor() as f32
                * state.scale_factor,
        };

        let surface_texture = match state.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => texture,
            CurrentSurfaceTexture::Suboptimal(texture) => {
                // optionally handle suboptimal
                texture
            }
            CurrentSurfaceTexture::Outdated => {
                // Ignoring outdated to allow resizing and minimization
                println!("wgpu surface outdated");
                return;
            }
            _ => {
                return;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            state.update_geom();

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            if state.draw2d.instance_count > 0 {
                pass.set_pipeline(&state.draw2d.pipeline);
                pass.set_bind_group(0, &state.draw2d.bind_group, &[]);
                pass.set_vertex_buffer(0, state.draw2d.quad_vbo.slice(..));
                pass.set_vertex_buffer(1, state.draw2d.instance_buffer.slice(..));
                pass.draw(0..6, 0..state.draw2d.instance_count);
            }
            if state.draw2d.circle_instance_count > 0 {
                pass.set_pipeline(&state.draw2d.circle_pipeline);
                pass.set_bind_group(0, &state.draw2d.bind_group, &[]);
                pass.set_vertex_buffer(0, state.draw2d.quad_vbo.slice(..));
                pass.set_vertex_buffer(1, state.draw2d.circle_instance_buf.slice(..));
                pass.draw(0..6, 0..state.draw2d.circle_instance_count);
            }
            /*
             */
            if state.draw2d.render_settings.debug && state.draw2d.debug_vertex_count > 0 {
                pass.set_pipeline(&state.draw2d.debug_pipeline);
                pass.set_bind_group(0, &state.draw2d.bind_group, &[]);
                pass.set_vertex_buffer(0, state.draw2d.debug_vbo.slice(..));
                pass.draw(0..state.draw2d.debug_vertex_count, 0..1);
            }
        }

        let window = self.window.as_ref().unwrap();
        {
            let egui_renderer = self.egui_renderer.as_mut().unwrap();
            egui_renderer.begin_frame(window);
            egui_renderer.build_ui(state);
            egui_renderer.end_frame_and_draw(
                &state.device,
                &state.queue,
                &mut encoder,
                window,
                &surface_view,
                screen_descriptor,
            );
        }

        state.queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
        self.state.as_mut().unwrap().init();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.egui_renderer
            .as_mut()
            .unwrap()
            .handle_input(self.window.as_ref().unwrap(), &event);
        match event {
            WindowEvent::CloseRequested => {
                self.state.as_mut().unwrap().sim.kill();
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && event.state.is_pressed()
                    && !event.repeat
                {
                    match code {
                        KeyCode::Space => {
                            let state = self.state.as_mut().unwrap();
                            if state.paused {
                                state.sim.resume();
                            } else {
                                state.sim.pause();
                            }
                            state.paused = !state.paused;
                        }
                        KeyCode::KeyR => {
                            self.state.as_mut().unwrap().sim.reset();
                        }
                        KeyCode::Escape => {
                            self.state.as_mut().unwrap().sim.kill();
                            event_loop.exit();
                        }
                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }
}
