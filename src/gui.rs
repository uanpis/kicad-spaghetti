use crate::app::AppState;
use crate::draw::{Draw2D, ScreenInfo};
use crate::utils::*;
use egui::{
    Align, Button, CentralPanel, CollapsingHeader, ComboBox, Context, Event, Frame, Grid, Layout,
    Panel, PointerButton, Popup, Pos2, Rect, RichText, ScrollArea, Separator, ThemePreference, Ui,
    UiBuilder, emath, emath::RectTransform, viewport::ViewportId, widgets::DragValue,
};
use egui_wgpu::{
    CallbackTrait, Renderer, RendererOptions, ScreenDescriptor, wgpu,
    wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureView},
};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use strum::IntoEnumIterator;
use winit::event::WindowEvent;
use winit::window::Window;

const FPS_INTERVAL_MS: u128 = 500;
const ROW_SPLIT: f32 = 0.3;

pub struct EguiRenderer {
    state: egui_winit::State,
    renderer: Renderer,
    frame_started: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, strum_macros::Display, strum_macros::EnumIter)]
pub enum ColorTheme {
    System,
    Light,
    Dark,
}

impl EguiRenderer {
    pub fn context(&self) -> &Context {
        self.state.egui_ctx()
    }

    pub fn new(state: &mut AppState, window: &Window) -> EguiRenderer {
        let egui_context = Context::default();

        let egui_state = egui_winit::State::new(
            egui_context,
            ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(2 * 1024), // default dimension is 2048
        );
        let mut egui_renderer = Renderer::new(
            &state.device,
            state.surface_config.format,
            RendererOptions::default(),
        );
        egui_renderer
            .callback_resources
            .insert(state.draw2d.clone());

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
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
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
        CentralPanel::no_frame().show(self.context(), |ui| {
            bottom_panel(ui, state);
            right_panel(ui, state);
            central_panel(ui, state);
        });
    }
}

struct Draw2DCallback {
    screen_size: [u32; 2],
    pan: glam::Vec2,
    zoom: f32,
}

impl CallbackTrait for Draw2DCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // set drawing surface size
        let draw2d_arc = callback_resources.get::<Arc<Mutex<Draw2D>>>().unwrap();
        let draw2d = draw2d_arc.lock().unwrap();
        let size = self.screen_size;

        draw2d.update_screen_info(
            queue,
            ScreenInfo {
                size,
                zoom: self.zoom,
                pan: self.pan.into(),
                aspect_ratio: size[0] as f32 / size[1] as f32,
            },
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let draw2d_arc = callback_resources.get::<Arc<Mutex<Draw2D>>>().unwrap();
        let draw2d = draw2d_arc.lock().unwrap();
        draw2d.render(render_pass);
    }
}

fn central_panel(ui: &mut Ui, state: &mut AppState) {
    CentralPanel::default().show_inside(ui, |ui| {
        let available = ui.available_size();
        let corner = ui.next_widget_position();

        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::drag());
        let relative_pointer_gesture = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    Event::MouseWheel { .. } | Event::Zoom { .. } | Event::PointerMoved(..)
                )
            })
        });
        let rect_proportions = response.rect.square_proportions();
        let to_screen = RectTransform::from_to(
            Rect::from_min_size(Pos2::ZERO - rect_proportions, 2. * rect_proportions),
            response.rect,
        );

        if ui.input(|i| i.multi_touch().is_some()) || relative_pointer_gesture {
            ui.input(|input| {
                let interact_pos = to_screen.inverse().scale()
                    * (input
                        .pointer
                        .interact_pos()
                        .map(|x| x.to_vec2())
                        .unwrap_or(0.5 * response.rect.size())
                        - 0.5 * response.rect.size());

                let pan_offset = to_screen.inverse().scale()
                    * (input.translation_delta()
                        + if input.pointer.button_down(PointerButton::Secondary)
                            || input.pointer.button_down(PointerButton::Middle)
                        {
                            input.pointer.delta()
                        } else {
                            egui::Vec2::ZERO
                        })
                    + interact_pos * (1.0 - input.zoom_delta());

                state.zoom *= input.zoom_delta();
                state.pan += glam::Vec2::new(pan_offset.x, pan_offset.y) / state.zoom;
            });
        }

        let snapshot = state.sim.get_snapshot();
        let z: f32 = 2.0 / snapshot.radius;
        let zoom = z * state.zoom;
        let pan = state.pan / z - snapshot.center;

        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            Draw2DCallback {
                screen_size: [available.x as u32, available.y as u32],
                zoom,
                pan,
            },
        ));

        let rect = Rect::from_min_size(corner, egui::vec2(available.x, 20.0));
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.with_layout(Layout::right_to_left(Align::RIGHT), |ui| {
                if ui
                    .add(
                        Button::new(if state.rightpanel { "▶" } else { "◀" })
                            .fill(ui.stack().bg_color()),
                    )
                    .clicked()
                {
                    state.rightpanel = !state.rightpanel;
                }
            });
        });
    });
}

fn bottom_panel(ui: &mut Ui, state: &mut AppState) {
    let button_size = egui::vec2(30.0, 30.0);
    let text_size = 15.0;
    Panel::bottom(egui::Id::new("bottom_panel"))
        .frame(Frame::central_panel(ui.style()).inner_margin(5.0))
        .resizable(false)
        .show_inside(ui, |ui| {
            let col_width = ui.available_width() / 2.0;
            Grid::new("bottom_panel_grid")
                .num_columns(2)
                .min_col_width(col_width)
                .max_col_width(col_width)
                .spacing([0.0, 0.0])
                .show(ui, |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if ui
                            .add_sized(button_size, Button::new(RichText::new("↺").size(text_size)),)
                            .on_hover_text("Reset [R]")
                            .clicked()
                        {
                            state.sim.reset();
                        }

                        if ui
                            .add_sized(
                                button_size,
                                Button::new(
                                    RichText::new(if state.paused { "▶" } else { "⏸" })
                                        .size(text_size),
                                ),
                            )
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

                    ui.with_layout(Layout::right_to_left(Align::RIGHT), |ui| {
                        if ui
                            .add_sized(
                                button_size,
                                Button::new(RichText::new("Cancel").size(text_size)),
                            )
                            .on_hover_text("Close without applying changes [Esc]")
                            .clicked()
                        {
                            state.exit_requested = true;
                        }
                        if ui
                            .add_sized(
                                button_size,
                                Button::new(RichText::new("Apply").size(text_size)),
                            )
                            .on_hover_text("Apply modifications and exit [Enter]")
                            .clicked()
                        {
                            //
                        }
                    })
                });
        });
}

fn right_panel(ui: &mut Ui, state: &mut AppState) {
    Panel::right(egui::Id::new("right_panel"))
        .resizable(true)
        .default_size(250.0)
        .size_range(100.0..=500.0)
        .show_animated_inside(ui, state.rightpanel, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                ComboBox::from_id_salt("presets_combo")
                    .width(ui.available_width())
                    .selected_text("Preset...")
                    .show_ui(ui, |ui| {
                        //TODO
                        /*
                        for variant in E::iter() {
                            ui.selectable_value(value.get_mut(), variant, variant.to_string());
                        }
                        */
                    })
                    .response
                    .on_hover_text("");

                ui.add(Separator::default().grow(8.0));
                sim_settings(ui, state);
                ui.add(Separator::default().grow(8.0));
                graphics_settings(ui, state);
                ui.add(Separator::default().grow(8.0));
                stats(ui, state);
                ui.add(Separator::default().grow(8.0));
                ui.vertical_centered(|ui| {
                    ui.hyperlink_to("source code", "https://github.com/uanpis/kicad-spaghetti");
                });
            });
        });
}

fn sim_settings(ui: &mut Ui, state: &mut AppState) {
    let mut changed = false;
    CollapsingHeader::new("Simulation")
        .default_open(true)
        .show(ui, |ui| {
            Grid::new("sim_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    changed |= bool_row(
                        ui,
                        &mut state.sim.sim_settings.fix_vias,
                        "Fix Vias",
                        "Lock free movement of vias",
                    );
                    //TODO implement
                    changed |= float_row(
                        ui,
                        &mut state.sim.sim_settings.damping,
                        "Damping",
                        "This does nothing for now :)",
                        "",
                        0.0..=1.0,
                    );
                    changed |= float_row(
                        ui,
                        &mut state.sim.sim_settings.segment_size,
                        "Segment Size",
                        "",
                        "",
                        2.0..=8.0,
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

fn repulsion_settings(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Repulsion")
        .default_open(true)
        .show(ui, |ui| {
            Grid::new("repulsion_settings_grid")
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

fn collision_settings(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Collision")
        .default_open(true)
        .show(ui, |ui| {
            Grid::new("collision_settings_grid")
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
                        1usize..=20,
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

fn sim_settings_extra(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut changed = false;
    CollapsingHeader::new("Advanced")
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("sim_settings_extra_grid")
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

fn graphics_settings(ui: &mut Ui, state: &mut AppState) {
    CollapsingHeader::new("Graphics")
        .default_open(false)
        .show(ui, |ui| {
            gui_settings(ui, state);
            debug_settings(ui, state);
        });
}

fn gui_settings(ui: &mut Ui, state: &mut AppState) {
    CollapsingHeader::new("Gui")
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("gui_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    if combo_row(
                        ui,
                        &mut state.color_theme,
                        "Color Theme",
                        "Select dark / light theme",
                    ) {
                        ui.ctx().set_theme(match state.color_theme.get() {
                            ColorTheme::System => ThemePreference::System,
                            ColorTheme::Light => ThemePreference::Light,
                            ColorTheme::Dark => ThemePreference::Dark,
                        });
                    }
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

fn debug_settings(ui: &mut Ui, state: &mut AppState) {
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
    let mut draw2d = state.draw2d.lock().unwrap();
    CollapsingHeader::new("Debug")
        .default_open(true)
        .show(ui, |ui| {
            Grid::new("gui_settings_grid")
                .num_columns(1)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    combo_row(
                        ui,
                        &mut draw2d.render_settings.color_mode,
                        "Color Mode",
                        "Rule to use for coloring edges",
                    );
                    bool_row(
                        ui,
                        &mut draw2d.render_settings.edge_mark,
                        "Highlight Collisions",
                        "Color Colliding Edges in Red",
                    );
                    bool_row(
                        ui,
                        &mut draw2d.render_settings.quadtree,
                        "Quadtree",
                        "Show Quadtree visualisation",
                    );
                    bool_row(
                        ui,
                        &mut draw2d.render_settings.nodebounds,
                        "Bounding Boxes",
                        "Show bounding boxes of Quadtree Nodes",
                    );
                    bool_row(
                        ui,
                        &mut draw2d.render_settings.mass_circles,
                        "Mass Circles",
                        "Show Circles with area corresponding to Quadtree Node accumulated mass",
                    );
                });
        });
}

fn stats(ui: &mut Ui, state: &mut AppState) {
    CollapsingHeader::new("Stats")
        .default_open(true)
        .show(ui, |ui| {
            let delta_t = state.time.elapsed().as_millis();
            if delta_t >= FPS_INTERVAL_MS {
                let i = state.sim.snapshot.iterations;
                if i > state.iterations {
                    let n = i - state.iterations;
                    state.fps = 1000.0 * n as f32 / delta_t as f32;
                }
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

fn bool_row(ui: &mut Ui, value: &mut BoolResettable, label: &str, tooltip: &str) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, "", |ui| {
        let response = ui.checkbox(value.get_mut(), label).on_hover_text(tooltip);
        Popup::context_menu(&response)
            .id(ui.make_persistent_id(label))
            .show(|ui| {
                if ui.button("Reset to Default").clicked() {
                    value.reset();
                    changed = true;
                    ui.close();
                }
            });
        changed |= response.changed();
    });
    changed
}

fn combo_row<R: Resettable<E>, E>(ui: &mut Ui, value: &mut R, label: &str, tooltip: &str) -> bool
where
    E: IntoEnumIterator + std::fmt::Display + Copy + PartialEq,
{
    let old_value = value.get();

    property_row(ui, ROW_SPLIT, label, |ui| {
        let response = ComboBox::from_id_salt(label)
            .width(ui.available_width())
            .selected_text(value.get().to_string())
            .show_ui(ui, |ui| {
                for variant in E::iter() {
                    ui.selectable_value(value.get_mut(), variant, variant.to_string());
                }
            })
            .response
            .on_hover_text(tooltip);

        Popup::context_menu(&response)
            .id(ui.make_persistent_id(label))
            .show(|ui| {
                if ui.button("Reset to Default").clicked() {
                    value.reset();
                    ui.close();
                }
            });
    });
    value.get() != old_value
}

fn float_row(
    ui: &mut Ui,
    value: &mut F32Resettable,
    label: &str,
    tooltip: &str,
    suffix: &str,
    range: core::ops::RangeInclusive<f32>,
) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = value.value;
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(&mut value.value)
                    .speed(0.005 * (*range.end() - *range.start()))
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{:.3}{}", x, suffix)),
            )
            .on_hover_text(tooltip);
        // right click menu
        Popup::context_menu(&response)
            .id(ui.make_persistent_id(label))
            .show(|ui| {
                if ui.button("Reset to Default").clicked() {
                    value.reset();
                    changed = true;
                    ui.close();
                }
            });
        if response.dragged() && range.contains(&before) {
            value.set(value.value.clamp(*range.start(), *range.end()));
        }
        changed |= response.changed();
    });
    changed
}

fn integer_row<R: Resettable<T>, T: num::Integer + num::NumCast + emath::Numeric>(
    ui: &mut Ui,
    value: &mut R,
    label: &str,
    tooltip: &str,
    suffix: &str,
    range: core::ops::RangeInclusive<T>,
) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = value.get();
        let value_i64 = <i64 as num::NumCast>::from::<T>(value.get()).unwrap();
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(value.get_mut())
                    .speed(0.05 * value_i64 as f32)
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{}{}", x, suffix)),
            )
            .on_hover_text(tooltip);
        if response.dragged() && range.contains(&before) {
            value.set(if value.get() <= *range.start() {
                *range.start()
            } else if value.get() > *range.end() {
                *range.end()
            } else {
                value.get()
            });
        }
        changed |= response.changed();
    });
    changed
}

fn percentage_row(ui: &mut Ui, value: &mut F32Resettable, label: &str, tooltip: &str) -> bool {
    let mut changed = false;
    property_row(ui, ROW_SPLIT, label, |ui| {
        let before = value.value;
        let response = ui
            .add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                DragValue::new(&mut value.value)
                    .speed(0.01)
                    .fixed_decimals(2)
                    .custom_formatter(|x, _| format!("{:>3.0}%", 100.0 * x))
                    .custom_parser(|s| s.parse::<f32>().map(|x| 0.01 * x as f64).ok()),
            )
            .on_hover_text(tooltip);
        // right click menu
        Popup::context_menu(&response)
            .id(ui.make_persistent_id(label))
            .show(|ui| {
                if ui.button("Reset to Default").clicked() {
                    value.reset();
                    changed = true;
                    ui.close();
                }
            });

        // soft clamp
        if response.dragged() && (0.0..=1.0).contains(&before) {
            value.set(value.value.clamp(0.0, 1.0));
        }
        changed |= response.changed();
    });
    changed
}

fn property_row(ui: &mut Ui, split: f32, label: &str, add_widget: impl FnOnce(&mut Ui)) {
    let spacing = ui.spacing().item_spacing.x;
    let width = ui.available_width();

    let lhs = width * split;
    let rhs = width - lhs - spacing;

    let height = ui.spacing().interact_size.y;

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(lhs, height),
            egui::Layout::right_to_left(Align::Center),
            |ui| {
                ui.label(label);
            },
        );

        ui.allocate_ui(egui::vec2(rhs, height), add_widget);
    });
    ui.end_row();
}
