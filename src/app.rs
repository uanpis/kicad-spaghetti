use crate::draw::*;
use crate::gui::EguiRenderer;
use crate::sim::Sim;
use egui::{
    CollapsingHeader,
    collapsing_header::CollapsingState,
    style::HandleShape,
    widgets::{Slider, SliderClamping, SliderOrientation},
};
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

const FPS_INTERVAL_MS: u128 = 500;

struct AppState {
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
    pub egui_renderer: EguiRenderer,
}

impl AppState {
    async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
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

        let egui_renderer = EguiRenderer::new(&device, surface_config.format, window);

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
            egui_renderer,
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
}

impl App {
    pub fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Self {
            instance,
            state: None,
            window: None,
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

        let state = AppState::new(
            &self.instance,
            surface,
            &window,
            initial_width,
            initial_width,
        )
        .await;

        self.window.get_or_insert(window);
        self.state.get_or_insert(state);
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
            state.egui_renderer.begin_frame(window);

            egui::CentralPanel::no_frame().show(state.egui_renderer.context(), |ui| {
                egui::Panel::top(egui::Id::new("top_panel"))
                    .resizable(false)
                    .show_inside(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.toggle_value(&mut state.leftpanel, "⬅");
                            if ui.button("↺").on_hover_text("Reset [R]").clicked() {
                                state.sim.reset();
                            }
                            if ui
                                .button(if state.paused { "⏵" } else { "⏸" })
                                .on_hover_text(if state.paused {
                                    "Run [Space]"
                                } else {
                                    "Pause [Space]"
                                })
                                .clicked()
                            {
                                if state.paused {
                                    state.sim.resume();
                                } else {
                                    state.sim.pause();
                                }
                                state.paused = !state.paused;
                            }
                        });
                    });
                egui::Panel::left(egui::Id::new("left_panel"))
                    .resizable(true)
                    .default_size(250.0)
                    .size_range(100.0..=500.0)
                    .show_animated_inside(ui, state.leftpanel, |ui| {
                        CollapsingHeader::new("Simulation")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.add(
                                    Slider::new(
                                        &mut state.sim.sim_settings.damping,
                                        (0.1)..=(10.0),
                                    )
                                    .logarithmic(true)
                                    .clamping(SliderClamping::Never)
                                    .smart_aim(true)
                                    .text("Damping")
                                    .trailing_fill(true)
                                    .handle_shape(HandleShape::Rect { aspect_ratio: 0.5 }),
                                );
                            });
                        CollapsingHeader::new("Graphics")
                            .default_open(false)
                            .show(ui, |ui| {
                                CollapsingHeader::new("UI")
                                    .default_open(false)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "Pixels per point: {}",
                                                state.egui_renderer.context().pixels_per_point()
                                            ));
                                            if ui.button("-").clicked() {
                                                state.scale_factor =
                                                    (state.scale_factor - 0.1).max(0.3);
                                            }
                                            if ui.button("+").clicked() {
                                                state.scale_factor =
                                                    (state.scale_factor + 0.1).min(3.0);
                                            }
                                        });
                                    });

                                CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    ui.make_persistent_id("DebugSettings"),
                                    false,
                                )
                                .show_header(ui, |ui| {
                                    ui.checkbox(&mut state.draw2d.render_settings.debug, "Debug");
                                })
                                .body(|ui| {
                                    ui.add_enabled_ui(state.draw2d.render_settings.debug, |ui| {
                                        ui.checkbox(
                                            &mut state.draw2d.render_settings.quadtree,
                                            "QuadTree",
                                        );
                                        ui.checkbox(
                                            &mut state.draw2d.render_settings.mass_circles,
                                            "Mass Circles",
                                        );
                                    })
                                });
                            });

                        CollapsingHeader::new("Stats")
                            .default_open(true)
                            .show(ui, |ui| {
                                let delta_t = state.time.elapsed().as_millis();
                                if delta_t >= FPS_INTERVAL_MS {
                                    let i = state.sim.snapshot.iterations;
                                    let n = i - state.iterations;
                                    state.fps = 1000.0 * n as f32 / delta_t as f32;
                                    state.iterations = i;
                                    state.time = Instant::now();
                                }

                                ui.label(format!("fps: {}", state.fps));
                                ui.label(format!("points: {}", state.sim.snapshot.points.len()));
                                ui.label(format!("edges: {}", state.sim.snapshot.edges.len()));
                            });
                        ui.add(egui::Separator::default().grow(8.0));
                        ui.vertical_centered(|ui| {
                            ui.label("Left Panel");
                        });
                    });
            });

            state.egui_renderer.end_frame_and_draw(
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
        self.state
            .as_mut()
            .unwrap()
            .egui_renderer
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
