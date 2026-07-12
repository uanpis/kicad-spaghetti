use crate::draw::*;
use crate::gui::{ColorTheme, EguiRenderer};
use crate::sim::Sim;
use crate::utils::*;
use egui_wgpu::{ScreenDescriptor, wgpu};
use glam::Vec2;
use std::sync::{Arc, Mutex};
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
    pub draw2d: Arc<Mutex<Draw2D>>,
    pub rightpanel: bool,

    pub exit_requested: bool,
    pub paused: bool,
    pub fps: f32,
    pub time: Instant,
    pub iterations: u64,

    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    pub scale_factor: F32Resettable,
    pub color_theme: ColorThemeResettable,
    pub zoom: f32,
    pub pan: Vec2,
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

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: surface_config.width,
                height: surface_config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        surface.configure(&device, &surface_config);

        let sim = Sim::new();
        let draw2d = Arc::new(Mutex::new(Draw2D::new(&device, &queue, selected_format)));

        let time = Instant::now();

        Self {
            sim,
            draw2d,

            rightpanel: true,
            exit_requested: false,
            paused: false,
            fps: 0.0,
            time,
            iterations: 0,

            device,
            queue,
            surface,
            surface_config,
            depth_texture,
            depth_view,

            scale_factor: 1.0.into(),
            color_theme: ColorTheme::System.into(),
            zoom: 1.0,
            pan: Vec2::ZERO,
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn update_geom(&mut self) {
        let snapshot = self.sim.get_snapshot();
        if snapshot.new {
            snapshot.new = false;
            let mut draw2d = self.draw2d.lock().unwrap();
            draw2d.rebuild(&self.device, &self.queue, snapshot);
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

        let mut state = AppState::new(&self.instance, surface, initial_width, initial_width).await;
        let egui_renderer = EguiRenderer::new(&mut state, &window);

        self.window.get_or_insert(window);
        self.state.get_or_insert(state);
        self.egui_renderer.get_or_insert(egui_renderer);
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        let state = self.state.as_mut().unwrap();
        if width > 0 && height > 0 {
            state.resize_surface(width, height);
        }

        // depth texture must match the new surface size exactly
        state.depth_texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        state.depth_view = state
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    fn handle_redraw(&mut self) {
        let state = self.state.as_mut().unwrap();
        state.update_geom();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: self.window.as_ref().unwrap().scale_factor() as f32
                * state.scale_factor.get(),
        };
        let surface_texture = match state.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => texture,
            CurrentSurfaceTexture::Suboptimal(texture) => texture,
            _ => {
                return;
            }
        };

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

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
                &texture_view,
                &state.depth_view,
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
        if self.state.as_ref().unwrap().exit_requested {
            event_loop.exit();
        }
    }
}
