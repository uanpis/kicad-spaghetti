use crate::colors::COPPER_COLORS;
use crate::sim::Snapshot;
use crate::utils::*;
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GpuVertex {
    pub position: [f32; 2],
    pub color: [f32; 4], // rgba
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ScreenInfo {
    pub size: [u32; 2],
    pub pan: [f32; 2],
    pub zoom: f32,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EdgeInstance {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub radius: f32,
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CircleInstance {
    pub center: [f32; 2],
    pub radius: f32,
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TriangleInstance {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub p2: [f32; 2],
    pub color: [f32; 4],
    pub radius: f32,
}

#[derive(Debug)]
pub struct Draw2D {
    pub render_settings: RenderSettings,

    pub screen_info_ubo: wgpu::Buffer,

    pub bind_group: wgpu::BindGroup,
    pub quad_vbo: wgpu::Buffer,
    pub tri_vbo: wgpu::Buffer,

    pub edge_pipeline: wgpu::RenderPipeline,
    pub edge_instance_buf: wgpu::Buffer,
    pub edge_instance_count: u32,

    pub circle_pipeline: wgpu::RenderPipeline,
    pub circle_instance_buf: wgpu::Buffer,
    pub circle_instance_count: u32,

    pub triangle_pipeline: wgpu::RenderPipeline,
    pub triangle_instance_buf: wgpu::Buffer,
    pub triangle_instance_count: u32,

    pub line_buf: wgpu::Buffer,
    pub line_count: u32,
    pub line_pipeline: wgpu::RenderPipeline,
}

#[derive(Clone, Copy, Debug, PartialEq, strum_macros::Display, strum_macros::EnumIter)]
pub enum ColorMode {
    //#[strum(to_string = "Layer (Default)")]
    Layer,
    Net,
    Curve,
    Edge,
}
impl_resettable!(ColorModeResettable, ColorMode);

#[derive(Debug)]
pub struct RenderSettings {
    pub quadtree: BoolResettable,
    pub nodebounds: BoolResettable,
    pub mass_circles: BoolResettable,
    pub color_mode: ColorModeResettable,
    pub edge_mark: BoolResettable,
}

impl Draw2D {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let render_settings = RenderSettings {
            quadtree: false.into(),
            nodebounds: false.into(),
            mass_circles: false.into(),
            color_mode: ColorMode::Layer.into(),
            edge_mark: false.into(),
        };

        const TRI_VERTS: &[[f32; 2]] = &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        const QUAD_VERTS: &[[f32; 2]] = &[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];

        // edge instance buffer
        let edge_instances = Vec::<EdgeInstance>::new();
        let edge_instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Edge Instance Buffer"),
            contents: bytemuck::cast_slice(&edge_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let edge_instance_count = edge_instances.len() as u32;

        // circle instance buffer
        let circle_instances = Vec::<CircleInstance>::new();
        let circle_instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Circle Instance Buffer"),
            contents: bytemuck::cast_slice(&circle_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let circle_instance_count = circle_instances.len() as u32;

        // triangle instance buffer
        let triangle_instances = Vec::<CircleInstance>::new();
        let triangle_instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Triangle Instance Buffer"),
            contents: bytemuck::cast_slice(&triangle_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let triangle_instance_count = triangle_instances.len() as u32;

        // quad buffer
        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Quad VBO"),
            contents: bytemuck::cast_slice(QUAD_VERTS),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // triangle buffer
        let tri_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Triangle VBO"),
            contents: bytemuck::cast_slice(TRI_VERTS),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // line buffer
        let line_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Debug VBO"),
            contents: bytemuck::bytes_of(&GpuVertex {
                position: [0.0, 0.0],
                color: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // screen info
        let screen_info_ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screen Info UBO"),
            size: std::mem::size_of::<ScreenInfo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &screen_info_ubo,
            0,
            bytemuck::cast_slice(&[ScreenInfo {
                size: [1024; 2],
                pan: [0.0, 0.0],
                zoom: 1.0,
                aspect_ratio: 1.0,
            }]),
        );

        // layout + bind
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ScreenInfo>() as u64
                    ),
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BG"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(screen_info_ubo.as_entire_buffer_binding()),
            }],
        });

        // shaders
        let edge_shader = device.create_shader_module(include_wgsl!("../shaders/edge_shader.wgsl"));
        let circle_shader =
            device.create_shader_module(include_wgsl!("../shaders/circle_shader.wgsl"));
        let triangle_shader =
            device.create_shader_module(include_wgsl!("../shaders/triangle_shader.wgsl"));
        let line_shader = device.create_shader_module(include_wgsl!("../shaders/line_shader.wgsl"));

        // pipelines
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        enum BlendMode {
            Mix,
            Max,
        }

        #[allow(clippy::too_many_arguments)]
        fn create_pipeline<'a>(
            device: &wgpu::Device,
            label: &str,
            layout: &wgpu::PipelineLayout,
            shader: &wgpu::ShaderModule,
            vertex_buffers: &[wgpu::VertexBufferLayout<'a>],
            topology: wgpu::PrimitiveTopology,
            format: wgpu::TextureFormat,
            blend_mode: BlendMode,
        ) -> wgpu::RenderPipeline {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    buffers: vertex_buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(match blend_mode {
                            BlendMode::Mix => wgpu::BlendState::ALPHA_BLENDING,
                            BlendMode::Max => {
                                wgpu::BlendState {
                                    color: wgpu::BlendComponent {
                                        src_factor: wgpu::BlendFactor::SrcAlpha,
                                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                        operation: wgpu::BlendOperation::Add,
                                    },
                                    // max alpha blend mode: fixes antialiasing overlap
                                    alpha: wgpu::BlendComponent {
                                        src_factor: wgpu::BlendFactor::One,
                                        dst_factor: wgpu::BlendFactor::One,
                                        operation: wgpu::BlendOperation::Max,
                                    },
                                }
                            }
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        }

        let quad_attrs = wgpu::vertex_attr_array![0 => Float32x2];
        let tri_attrs = wgpu::vertex_attr_array![0 => Float32x2];
        let edge_instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2,  // p0
            2 => Float32x2,  // p1
            3 => Float32,    // radius
            4 => Float32x4,  // color
        ];
        let circle_instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2, // center
            2 => Float32,   // radius
            3 => Float32x4, // color
        ];
        let triangle_instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2, // p0
            2 => Float32x2, // p1
            3 => Float32x2, // p2
            4 => Float32x4, // color
            5 => Float32,   // radius
        ];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

        let edge_pipeline = create_pipeline(
            device,
            "Edge SDF Pipeline",
            &pipeline_layout,
            &edge_shader,
            &[
                wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &quad_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<EdgeInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &edge_instance_attrs,
                },
            ],
            wgpu::PrimitiveTopology::TriangleList,
            surface_format,
            BlendMode::Max,
        );

        let circle_pipeline = create_pipeline(
            device,
            "Circle SDF Pipeline",
            &pipeline_layout,
            &circle_shader,
            &[
                wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &quad_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CircleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &circle_instance_attrs,
                },
            ],
            wgpu::PrimitiveTopology::TriangleList,
            surface_format,
            BlendMode::Max,
        );

        let triangle_pipeline = create_pipeline(
            device,
            "Triangle SDF Pipeline",
            &pipeline_layout,
            &triangle_shader,
            &[
                wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &tri_attrs,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TriangleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &triangle_instance_attrs,
                },
            ],
            wgpu::PrimitiveTopology::TriangleList,
            surface_format,
            BlendMode::Max,
        );

        let line_pipeline = create_pipeline(
            device,
            "Line Pipeline",
            &pipeline_layout,
            &line_shader,
            &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &line_attrs,
            }],
            wgpu::PrimitiveTopology::LineList,
            surface_format,
            BlendMode::Mix,
        );

        Self {
            render_settings,

            screen_info_ubo,
            bind_group,

            quad_vbo,
            tri_vbo,

            edge_pipeline,
            edge_instance_buf,
            edge_instance_count,

            circle_pipeline,
            circle_instance_buf,
            circle_instance_count,

            triangle_pipeline,
            triangle_instance_buf,
            triangle_instance_count,

            line_buf,
            line_pipeline,
            line_count: 0,
        }
    }

    pub fn update_screen_info(&self, queue: &wgpu::Queue, screen_info: ScreenInfo) {
        queue.write_buffer(
            &self.screen_info_ubo,
            0,
            bytemuck::cast_slice(&[screen_info]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass) {
        if self.edge_instance_count > 0 {
            render_pass.set_pipeline(&self.edge_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
            render_pass.set_vertex_buffer(1, self.edge_instance_buf.slice(..));
            render_pass.draw(0..6, 0..self.edge_instance_count);
        }
        if self.circle_instance_count > 0 {
            render_pass.set_pipeline(&self.circle_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
            render_pass.set_vertex_buffer(1, self.circle_instance_buf.slice(..));
            render_pass.draw(0..6, 0..self.circle_instance_count);
        }
        if self.triangle_instance_count > 0 {
            render_pass.set_pipeline(&self.triangle_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.tri_vbo.slice(..));
            render_pass.set_vertex_buffer(1, self.triangle_instance_buf.slice(..));
            render_pass.draw(0..3, 0..self.triangle_instance_count);
        }
        if self.line_count > 0 {
            render_pass.set_pipeline(&self.line_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.line_buf.slice(..));
            render_pass.draw(0..self.line_count, 0..1);
        }
    }

    pub fn _render_to_texture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });

        self.render(&mut pass);
    }

    pub fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, snapshot: &Snapshot) {
        let mut colors = vec![[0.0, 0.0, 0.0, 0.0]; COPPER_COLORS.len()];
        snapshot
            .layer_map
            .iter()
            .for_each(|(key, i)| colors[*i] = COPPER_COLORS[*key as usize]);

        let mut instances = build_edge_instances(
            snapshot,
            self.render_settings.color_mode.get(),
            self.render_settings.edge_mark.get(),
            &colors,
        );
        //if self.render_settings.
        instances.extend(build_footprint_outlines(snapshot));

        let count = instances.len() as u32;
        if count == self.edge_instance_count {
            queue.write_buffer(&self.edge_instance_buf, 0, bytemuck::cast_slice(&instances));
        } else {
            self.edge_instance_count = count;
            self.edge_instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Edge Instance Buffer"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
        }

        let mut circle_instances = Vec::<CircleInstance>::new();
        circle_instances.extend(build_circle_pads(
            snapshot,
            self.render_settings.color_mode.get(),
            &colors,
        ));
        if self.render_settings.mass_circles.get() {
            circle_instances.extend(build_mass_circles(snapshot));
        }
        //circle_instances.extend(build_point_circles(snapshot));

        let circle_count = circle_instances.len() as u32;
        if circle_count == self.circle_instance_count {
            queue.write_buffer(
                &self.circle_instance_buf,
                0,
                bytemuck::cast_slice(&circle_instances),
            );
        } else {
            self.circle_instance_count = circle_count;
            self.circle_instance_buf =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Circle Instance Buffer"),
                    contents: bytemuck::cast_slice(&circle_instances),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
        }

        let triangle_instances = build_polygon_triangles(snapshot, &colors);
        let triangle_count = triangle_instances.len() as u32;
        if triangle_count == self.triangle_instance_count {
            queue.write_buffer(
                &self.triangle_instance_buf,
                0,
                bytemuck::cast_slice(&triangle_instances),
            );
        } else {
            self.triangle_instance_count = triangle_count;
            self.triangle_instance_buf =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Triangle Instance Buffer"),
                    contents: bytemuck::cast_slice(&triangle_instances),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
        }

        let mut vertices = Vec::<GpuVertex>::new();
        if self.render_settings.quadtree.get() {
            vertices.extend(build_debug_tree(snapshot));
        }
        if self.render_settings.nodebounds.get() {
            vertices.extend(build_node_bounds(snapshot, &colors));
        }

        self.line_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Line VBO"),
            contents: if vertices.is_empty() {
                bytemuck::bytes_of(&GpuVertex {
                    position: [0.0, 0.0],
                    color: [0.0; 4],
                })
            } else {
                bytemuck::cast_slice(&vertices)
            },
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.line_count = vertices.len() as u32;
    }
}

fn build_edge_instances(
    snapshot: &Snapshot,
    color_mode: ColorMode,
    mark: bool,
    colors: &[[f32; 4]],
) -> Vec<EdgeInstance> {
    //let color_mark = [1.0; 4];
    snapshot
        .curves
        .iter()
        .enumerate()
        .flat_map(|(i, curve)| {
            let k = snapshot.points[curve[0].i0].net;
            curve.iter().enumerate().map(move |(j, edge)| {
                let mut color = {
                    fn hue(seed: usize) -> [f32; 4] {
                        let t = 2.399963 * seed as f32;
                        [
                            0.5 + 0.5 * t.sin(),
                            0.5 + 0.5 * (t + 2.0943951).sin(),
                            0.5 + 0.5 * (t + 4.1887902).sin(),
                            1.0,
                        ]
                    }
                    match color_mode {
                        ColorMode::Layer => colors[snapshot.points[edge.i0].layer],
                        ColorMode::Net => hue(k),
                        ColorMode::Edge => hue(j),
                        ColorMode::Curve => hue(i),
                    }
                };
                if mark && edge.mark {
                    let one = glam::f32::vec4(1.0, 1.0, 1.0, 1.0);
                    let col: glam::f32::Vec4 = color.into();
                    color = (one - 0.5 * (one - col)).into();
                }
                let p0 = snapshot.points[edge.i0].pos;
                let p1 = snapshot.points[edge.i1].pos;
                EdgeInstance {
                    p0: [p0.x, p0.y],
                    p1: [p1.x, p1.y],
                    radius: edge.w * 0.5,
                    color,
                }
            })
        })
        .collect()
}

fn _build_point_circles(snapshot: &Snapshot) -> Vec<CircleInstance> {
    snapshot
        .points
        .iter()
        .map(|point| CircleInstance {
            center: point.pos.into(),
            radius: point.rad,
            color: [0.8, 0.2, 0.1, 1.0],
        })
        .collect()
}

fn build_mass_circles(snapshot: &Snapshot) -> Vec<CircleInstance> {
    snapshot
        .trees
        .iter()
        .flat_map(|tree| &tree.nodes)
        .map(|node| CircleInstance {
            center: [node.data.pos.x, node.data.pos.y],
            radius: node.data.mass.sqrt(),
            color: [0.8, 0.2, 0.1, 0.4],
        })
        .collect()
}

fn build_circle_pads(
    snapshot: &Snapshot,
    color_mode: ColorMode,
    colors: &[[f32; 4]],
) -> Vec<CircleInstance> {
    snapshot
        .vias
        .iter()
        .flat_map(|via| via.attached_points.iter())
        .chain(
            snapshot
                .footprints
                .iter()
                .flat_map(|fp| fp.attached_points.iter()),
        )
        .map(|i| {
            let color = {
                fn hue(seed: usize) -> [f32; 4] {
                    let t = 2.399963 * seed as f32;
                    [
                        0.5 + 0.5 * t.sin(),
                        0.5 + 0.5 * (t + 2.0943951).sin(),
                        0.5 + 0.5 * (t + 4.1887902).sin(),
                        1.0,
                    ]
                }
                match color_mode {
                    ColorMode::Layer => colors[snapshot.points[*i].layer],
                    ColorMode::Net => hue(snapshot.points[*i].net),
                    _ => colors[snapshot.points[*i].layer],
                }
            };
            let point = &snapshot.points[*i];
            CircleInstance {
                center: point.pos.into(),
                radius: point.rad,
                color,
            }
        })
        .collect()
}

fn build_node_bounds(snapshot: &Snapshot, colors: &[[f32; 4]]) -> Vec<GpuVertex> {
    snapshot
        .trees
        .iter()
        .enumerate()
        .flat_map(|(i, tree)| {
            tree.nodes.iter().flat_map(move |node| {
                let bounds = &node.data.aabb;
                let mut color = colors[i];
                color[3] = 0.2;
                let edge = |s0: usize, s1: usize, e0: usize, e1: usize| -> [GpuVertex; 2] {
                    [
                        GpuVertex {
                            position: [bounds[s0], bounds[s1]],
                            color,
                        },
                        GpuVertex {
                            position: [bounds[e0], bounds[e1]],
                            color,
                        },
                    ]
                };
                [
                    edge(0, 1, 0, 3),
                    edge(0, 3, 2, 3),
                    edge(2, 3, 2, 1),
                    edge(2, 1, 0, 1),
                ]
                .into_iter()
                .flatten()
            })
        })
        .collect()
}

fn build_polygon_triangles(snapshot: &Snapshot, colors: &[[f32; 4]]) -> Vec<TriangleInstance> {
    snapshot
        .polygons
        .iter()
        .flat_map(|polygon| {
            let layer = polygon.layer;
            let radius = polygon.rad;
            polygon.triangulation.iter().map(move |tri| {
                let i0 = polygon.points[tri[0]];
                let i1 = polygon.points[tri[1]];
                let i2 = polygon.points[tri[2]];
                let p0 = snapshot.points[i0].pos.into();
                let p1 = snapshot.points[i1].pos.into();
                let p2 = snapshot.points[i2].pos.into();
                TriangleInstance {
                    p0,
                    p1,
                    p2,
                    color: colors[layer],
                    radius,
                }
            })
        })
        .collect()
}

fn build_footprint_outlines(snapshot: &Snapshot) -> Vec<EdgeInstance> {
    let color_front = hex_color(0xFF26E2FF);
    let color_back = hex_color(0x26E9FFFF);
    snapshot
        .footprints
        .iter()
        .flat_map(|fp| {
            fp.outlines
                .iter()
                .map(|x| (x, fp.pos, glam::Mat2::from_angle(fp.rot)))
        })
        .flat_map(|(outline, pos, transform)| {
            let mut v = Vec::<EdgeInstance>::new();
            let layer = outline.layer;
            let len = outline.points.len();
            for i in 0..len {
                let point = &outline.points[i];
                let next = &outline.points[(i + 1) % len];
                v.push(EdgeInstance {
                    p0: (transform.mul_vec2(point.pos) + pos).into(),
                    p1: (transform.mul_vec2(next.pos) + pos).into(),
                    radius: point.w,
                    color: if layer { color_front } else { color_back },
                });
            }
            v
        })
        .collect()
}

fn build_debug_tree(snapshot: &Snapshot) -> Vec<GpuVertex> {
    let color = [0.5, 0.5, 0.5, 0.2];
    snapshot
        .trees
        .iter()
        .flat_map(|tree| tree.get_viz())
        .flat_map(|v| {
            [
                GpuVertex {
                    position: v[0].into(),
                    color,
                },
                GpuVertex {
                    position: v[1].into(),
                    color,
                },
            ]
        })
        .collect()
}

unsafe impl bytemuck::Pod for ScreenInfo {}
unsafe impl bytemuck::Zeroable for ScreenInfo {}
unsafe impl bytemuck::Pod for GpuVertex {}
unsafe impl bytemuck::Zeroable for GpuVertex {}
unsafe impl bytemuck::Pod for EdgeInstance {}
unsafe impl bytemuck::Zeroable for EdgeInstance {}
unsafe impl bytemuck::Pod for CircleInstance {}
unsafe impl bytemuck::Zeroable for CircleInstance {}
unsafe impl bytemuck::Pod for TriangleInstance {}
unsafe impl bytemuck::Zeroable for TriangleInstance {}
