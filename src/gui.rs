use crate::app::AppState;
use egui::{CollapsingHeader, Context, widgets::DragValue};
use egui_wgpu::wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureFormat, TextureView};
use egui_wgpu::{Renderer, ScreenDescriptor, wgpu};
use egui_winit::State;
use std::time::Instant;
use strum::IntoEnumIterator;
use winit::event::WindowEvent;
use winit::window::Window;

const FPS_INTERVAL_MS: u128 = 500;
const ROW_SPLIT: f32 = 0.3;

pub struct EguiRenderer {
    state: State,
    renderer: Renderer,
    frame_started: bool,
}

impl EguiRenderer {
    pub fn context(&self) -> &Context {
        self.state.egui_ctx()
    }

    pub fn new(
        device: &Device,
        output_color_format: TextureFormat,
        window: &Window,
    ) -> EguiRenderer {
        let egui_context = Context::default();

        let egui_state = egui_winit::State::new(
            egui_context,
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(2 * 1024), // default dimension is 2048
        );
        let egui_renderer = Renderer::new(
            device,
            output_color_format,
            egui_wgpu::RendererOptions::default(),
        );

        EguiRenderer {
            state: egui_state,
            renderer: egui_renderer,
            frame_started: false,
        }
    }

    pub fn handle_input(&mut self, window: &Window, event: &WindowEvent) {
        let _ = self.state.on_window_event(window, event);
    }

    pub fn ppp(&mut self, v: f32) {
        self.context().set_pixels_per_point(v);
    }

    pub fn begin_frame(&mut self, window: &Window) {
        let raw_input = self.state.take_egui_input(window);
        self.state.egui_ctx().begin_pass(raw_input);
        self.frame_started = true;
    }

    pub fn end_frame_and_draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window: &Window,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw can be called!");
        }

        self.ppp(screen_descriptor.pixels_per_point);

        let full_output = self.state.egui_ctx().end_pass();

        self.state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .state
            .egui_ctx()
            .tessellate(full_output.shapes, self.state.egui_ctx().pixels_per_point());
        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &tris, &screen_descriptor);
        let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: egui_wgpu::wgpu::Operations {
                    load: egui_wgpu::wgpu::LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            label: Some("egui main render pass"),
            occlusion_query_set: None,
            multiview_mask: None,
        });

        self.renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x)
        }

        self.frame_started = false;
    }

    pub fn build_ui(&mut self, state: &mut AppState) {
        #[allow(deprecated)]
        egui::CentralPanel::no_frame().show(self.context(), |ui| {
            top_panel(ui, state);
            left_panel(ui, state);
        });
    }
}

fn top_panel(ui: &mut egui::Ui, state: &mut AppState) {
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
}

fn left_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::left(egui::Id::new("left_panel"))
        .resizable(true)
        .default_size(250.0)
        .size_range(100.0..=500.0)
        .show_animated_inside(ui, state.leftpanel, |ui| {
            sim_settings(ui, state);
            graphics_settings(ui, state);
            stats(ui, state);
            ui.add(egui::Separator::default().grow(8.0));
            ui.vertical_centered(|ui| {
                ui.label("Left Panel");
            });
        });
}

fn sim_settings(ui: &mut egui::Ui, state: &mut AppState) {
    let mut changed = false;
    CollapsingHeader::new("Simulation")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("sim_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    //TODO implement
                    changed |= float_row(
                        ui,
                        &mut state.sim.sim_settings.damping,
                        "Damping",
                        "This does nothing for now :)",
                        "",
                        0.0..=1.0,
                    );

                    changed |= percentage_row(
                        ui,
                        &mut state.sim.sim_settings.noodliness,
                        "Noodliness",
                        "Repulsion / Tension ratio",
                    );
                });
            changed |= repulsion_settings(ui, state);
            changed |= collision_settings(ui, state);
            changed |= sim_settings_extra(ui, state);
        });

    if changed {
        state.sim.update_settings();
    }
}

fn repulsion_settings(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Repulsion")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("repulsion_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    changed |= integer_row(
                        ui,
                        &mut state.sim.sim_settings.repulsion_degree,
                        "Degree",
                        "Exponent \'n\' in force calculation: \'1/d^n\'",
                        "",
                        1..=6,
                    );
                    changed |= bool_row(
                        ui,
                        &mut state.sim.sim_settings.self_repulsion,
                        "Self repulsion",
                        "Enable repulsion between objects in the same net",
                    );
                });
        });
    changed
}

fn collision_settings(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Collision")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("collision_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    changed |= percentage_row(
                        ui,
                        &mut state.sim.sim_settings.collision_elasticity,
                        "Elasticity",
                        "Bounciness of collisions between objects",
                    );
                    changed |= integer_row(
                        ui,
                        &mut state.sim.sim_settings.collision_iterations,
                        "Iterations",
                        "Number of iterations done by the collision solver",
                        "",
                        1usize..=10,
                    );
                    changed |= bool_row(
                        ui,
                        &mut state.sim.sim_settings.self_collision,
                        "Self collision",
                        "Enable collision between objects in the same net",
                    );
                });
        });
    changed
}

fn sim_settings_extra(ui: &mut egui::Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Advanced")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("sim_settings_extra_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    changed |= bool_row(
                        ui,
                        &mut state.sim.sim_settings.limit_step,
                        "Limit step size",
                        "Clamp step size to half of the smallest track width",
                    );
                });
        });
    changed
}

fn graphics_settings(ui: &mut egui::Ui, state: &mut AppState) {
    CollapsingHeader::new("Graphics")
        .default_open(false)
        .show(ui, |ui| {
            gui_settings(ui, state);
            debug_settings(ui, state);
        });
}

fn gui_settings(ui: &mut egui::Ui, state: &mut AppState) {
    CollapsingHeader::new("Gui")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("gui_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    //TODO implement
                    float_row(
                        ui,
                        &mut state.scale_factor,
                        "Scaling Factor",
                        "Number of Pixels per point",
                        "",
                        0.3..=3.0,
                    );
                });
        });
}

fn debug_settings(ui: &mut egui::Ui, state: &mut AppState) {
    /*
    CollapsingState::load_with_default_open(
        ui.ctx(),
        ui.make_persistent_id("DebugSettings"),
        false,
    )
    .show_header(ui, |ui| {
        ui.checkbox(&mut state.draw2d.render_settings.debug, "Debug");
    })
    .body(|ui| {
    */
    CollapsingHeader::new("Debug")
        .default_open(true)
        .show(ui, |ui| {
            ui.add_enabled_ui(state.draw2d.render_settings.debug, |ui| {
                egui::Grid::new("gui_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    combo_row::<crate::draw::ColorMode>(
                        ui,
                        &mut state.draw2d.render_settings.color_mode,
                        "Color Mode",
                        "Rule to use for coloring edges",
                    );
                    bool_row(
                        ui,
                        &mut state.draw2d.render_settings.edge_mark,
                        "Highlight Collisions",
                        "Color Colliding Edges in Red",
                    );
                    bool_row(
                        ui,
                        &mut state.draw2d.render_settings.quadtree,
                        "Quadtree",
                        "Show Quadtree visualisation",
                    );
                    bool_row(
                        ui,
                        &mut state.draw2d.render_settings.nodebounds,
                        "Bounding Boxes",
                        "Show bounding boxes of Quadtree Nodes",
                    );
                    bool_row(
                        ui,
                        &mut state.draw2d.render_settings.mass_circles,
                        "Mass Circles",
                        "Show Circles with area corresponding to Quadtree Node accumulated mass",
                    );
                });
            });
        });
}

fn stats(ui: &mut egui::Ui, state: &mut AppState) {
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
            ui.label(format!("curves: {}", state.sim.snapshot.curves.len()));
            ui.label(format!(
                "edges: {}",
                state.sim.snapshot.curves.iter().flatten().count()
            ));
        });
}

fn bool_row(ui: &mut egui::Ui, value: &mut bool, label: &str, tooltip: &str) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, "", |ui| {
        changed = ui.checkbox(value, label).on_hover_text(tooltip).changed();
    });
    changed
}

fn combo_row<E>(ui: &mut egui::Ui, value: &mut E, label: &str, tooltip: &str) -> bool
where
    E: IntoEnumIterator + std::fmt::Display + Copy + PartialEq,
{
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let response = egui::ComboBox::from_id_salt(label)
            .width(ui.available_width())
            .selected_text(value.to_string())
            .show_ui(ui, |ui| {
                for variant in E::iter() {
                    ui.selectable_value(value, variant, variant.to_string());
                }
            })
            .response
            .on_hover_text(tooltip);
        changed = response.changed();
    });
    changed
}

fn float_row(
    ui: &mut egui::Ui,
    value: &mut f32,
    label: &str,
    tooltip: &str,
    suffix: &str,
    range: core::ops::RangeInclusive<f32>,
) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = *value;
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(value)
                    .speed(0.005 * (*range.end() - *range.start()))
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{:.3}{}", x, suffix)),
            )
            .on_hover_text(tooltip);
        if response.dragged() && range.contains(&before) {
            *value = value.clamp(*range.start(), *range.end());
        }
        changed = response.changed();
    });
    changed
}

fn integer_row<T: num::Integer + num::NumCast + emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    label: &str,
    tooltip: &str,
    suffix: &str,
    range: core::ops::RangeInclusive<T>,
) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = *value;
        let value_i64 = <i64 as num::NumCast>::from::<T>(*value).unwrap();
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(value)
                    .speed(0.05 * value_i64 as f32)
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{}{}", x, suffix)),
            )
            .on_hover_text(tooltip);
        if response.dragged() && range.contains(&before) {
            *value = if *value <= *range.start() {
                *range.start()
            } else if *value > *range.end() {
                *range.end()
            } else {
                *value
            };
        }
        changed = response.changed();
    });
    changed
}

fn percentage_row(ui: &mut egui::Ui, value: &mut f32, label: &str, tooltip: &str) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = *value;
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(value)
                    .speed(0.01)
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{:>3.0}%", 100.0 * x))
                    .custom_parser(|s| s.parse::<f32>().map(|x| 0.01 * x as f64).ok()),
            )
            .on_hover_text(tooltip);
        if response.dragged() && (0.0..=1.0).contains(&before) {
            *value = value.clamp(0.0, 1.0);
        }
        changed = response.changed();
    });
    changed
}

fn property_row(
    ui: &mut egui::Ui,
    split: f32,
    label: &str,
    add_widget: impl FnOnce(&mut egui::Ui),
) {
    let spacing = ui.spacing().item_spacing.x;
    let width = ui.available_width();

    let lhs = width * split;
    let rhs = width - lhs - spacing;

    let height = ui.spacing().interact_size.y;

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(lhs, height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );

        ui.allocate_ui(egui::vec2(rhs, height), add_widget);
    });
    ui.end_row();
}
