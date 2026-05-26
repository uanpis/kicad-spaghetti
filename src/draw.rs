use crate::sim::Snapshot;
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct GpuVertex {
    pub position: [f32; 2],
    pub color: [f32; 4], // rgba
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct GpuGlobals {
    pub zoom: [f32; 2],
    pub pan: [f32; 2],
    pub aspect_ratio: f32,
    pub _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct EdgeInstance {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub radius: f32,
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CircleInstance {
    pub center: [f32; 2],
    pub radius: f32,
    pub _pad: f32,
    pub color: [f32; 4],
}

#[derive(Debug)]
pub struct Draw2D {
    pub globals_ubo: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,

    pub pipeline: wgpu::RenderPipeline,
    pub quad_vbo: wgpu::Buffer,
    pub instance_buffer: wgpu::Buffer,
    pub instance_count: u32,

    pub circle_pipeline: wgpu::RenderPipeline,
    pub circle_instance_buf: wgpu::Buffer,
    pub circle_instance_count: u32,

    pub debug: bool,
    pub debug_vbo: wgpu::Buffer,
    pub debug_vertex_count: u32,
    pub debug_pipeline: wgpu::RenderPipeline,
}

impl Draw2D {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        snapshot: &Snapshot,
    ) -> Self {
        const QUAD_VERTS: &[[f32; 2]] = &[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];

        // buffers
        let instances = build_edge_instances(snapshot);
        let circle_instances = build_circle_instances(snapshot);
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Edge Instance Buffer"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let instance_count = instances.len() as u32;
        let circle_instance_count = circle_instances.len() as u32;

        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Edge Quad VBO"),
            contents: bytemuck::cast_slice(QUAD_VERTS),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let circle_instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Circle Instance Buffer"),
            contents: bytemuck::cast_slice(&circle_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let debug_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Debug VBO"),
            contents: bytemuck::bytes_of(&GpuVertex {
                position: [0.0, 0.0],
                color: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // zoom/pan/aspect
        let globals_ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Globals UBO"),
            size: std::mem::size_of::<GpuGlobals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &globals_ubo,
            0,
            bytemuck::cast_slice(&[GpuGlobals {
                zoom: [1.0, 1.0],
                pan: [0.0, 0.0],
                aspect_ratio: 1.0,
                _pad: 0.0,
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
                        std::mem::size_of::<GpuGlobals>() as u64
                    ),
                },
                count: None,
            }],
        });

        let circle_instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2,   // center
            2 => Float32,     // radius
            3 => Float32x4,   // color   (location 3; _pad is skipped by wgpu automatically
        ];

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("BG"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(globals_ubo.as_entire_buffer_binding()),
            }],
        });

        // shaders
        let edge_shader = device.create_shader_module(include_wgsl!("../shaders/edge_shader.wgsl"));
        let circle_shader =
            device.create_shader_module(include_wgsl!("../shaders/circle_shader.wgsl"));
        let debug_shader =
            device.create_shader_module(include_wgsl!("../shaders/debug_shader.wgsl"));

        // pipelines
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        fn create_pipeline<'a>(
            device: &wgpu::Device,
            label: &str,
            layout: &wgpu::PipelineLayout,
            shader: &wgpu::ShaderModule,
            vertex_buffers: &[wgpu::VertexBufferLayout<'a>],
            topology: wgpu::PrimitiveTopology,
            format: wgpu::TextureFormat,
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
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
        let circle_instance_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 1,
            }, // center
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: 8,
                shader_location: 2,
            }, // radius
            // offset 12 = _pad (4 bytes, skipped)
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 3,
            }, // color
        ];
        let instance_attrs = wgpu::vertex_attr_array![
            1 => Float32x2,  // p0
            2 => Float32x2,  // p1
            3 => Float32,    // radius
            4 => Float32x4,  // color
        ];
        let debug_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

        let pipeline = create_pipeline(
            device,
            "Edge/Node SDF Pipeline",
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
                    attributes: &instance_attrs,
                },
            ],
            wgpu::PrimitiveTopology::TriangleList,
            surface_format,
        );

        let circle_pipeline = create_pipeline(
            device,
            "Circle SDF Pipeline",
            &pipeline_layout, // reuses the same layout (same bind group)
            &circle_shader,
            &[
                wgpu::VertexBufferLayout {
                    // slot 0 — shared unit quad
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &quad_attrs,
                },
                wgpu::VertexBufferLayout {
                    // slot 1 — per-circle instances
                    array_stride: std::mem::size_of::<CircleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &circle_instance_attrs,
                },
            ],
            wgpu::PrimitiveTopology::TriangleList,
            surface_format,
        );

        let debug_pipeline = create_pipeline(
            device,
            "Debug Pipeline",
            &pipeline_layout,
            &debug_shader,
            &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &debug_attrs,
            }],
            wgpu::PrimitiveTopology::LineList,
            surface_format,
        );

        Self {
            globals_ubo,
            bind_group,

            pipeline,
            quad_vbo,
            instance_buffer,
            instance_count,

            circle_pipeline,
            circle_instance_buf,
            circle_instance_count,

            debug: true,
            debug_vbo,
            debug_vertex_count: 0,
            debug_pipeline,
        }
    }

    pub fn update_globals(&self, queue: &wgpu::Queue, globals: GpuGlobals) {
        queue.write_buffer(&self.globals_ubo, 0, bytemuck::cast_slice(&[globals]));
    }

    pub fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, snapshot: &Snapshot) {
        let instances = build_edge_instances(snapshot);
        let count = instances.len() as u32;
        if count == self.instance_count {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        } else {
            self.instance_count = count;
            self.instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Edge Instance Buffer"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
        }

        let circle_instances = build_circle_instances(snapshot);
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

        if self.debug {
            let vertices = build_debug_vertices(snapshot);
            self.debug_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Debug VBO"),
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

            self.debug_vertex_count = vertices.len() as u32;
        }
    }
}

fn build_edge_instances(snapshot: &Snapshot) -> Vec<EdgeInstance> {
    let color = [0.2, 0.2, 0.8, 1.0f32];

    snapshot
        .edges
        .iter()
        .map(|edge| {
            let p0 = snapshot.points[edge.i0].pos;
            let p1 = snapshot.points[edge.i1].pos;
            EdgeInstance {
                p0: [p0.x, p0.y],
                p1: [p1.x, p1.y],
                radius: edge.w * 0.5,
                color,
            }
        })
        .collect()
}

fn build_circle_instances(snapshot: &Snapshot) -> Vec<CircleInstance> {
    if let Some(tree) = &snapshot.tree {
        let mut points = Vec::<CircleInstance>::new();
        for node in tree.nodes.iter() {
            if let Some(idx) = node.data {
                let nodedata = &tree.data[idx];
                points.push(CircleInstance {
                    center: [nodedata.sum_all.pos.x, nodedata.sum_all.pos.y],
                    //center: [node.pos.x, node.pos.y],
                    //radius: 0.3 * nodedata.sum_all.mass.sqrt(),
                    radius: (node.len as f32).sqrt(),
                    _pad: 0.0,
                    color: [0.9, 0.4, 0.1, 0.10],
                })
            }
        }
        points
    } else {
        vec![CircleInstance {
            center: [0.0, 0.0],
            radius: 0.0,
            _pad: 0.0,
            color: [0.0, 0.0, 0.0, 0.0],
        }]
    }
}

fn build_debug_vertices(snapshot: &Snapshot) -> Vec<GpuVertex> {
    let color = [0.5, 0.5, 0.5, 0.2];
    if let Some(tree) = &snapshot.tree {
        tree.get_viz()
            .iter()
            .flat_map(|v| {
                [
                    GpuVertex {
                        position: [v[0].x, v[0].y],
                        color,
                    },
                    GpuVertex {
                        position: [v[1].x, v[1].y],
                        color,
                    },
                ]
            })
            .collect()
    } else {
        vec![GpuVertex {
            position: [0.0, 0.0],
            color: [0.0; 4],
        }]
    }
}

unsafe impl bytemuck::Pod for GpuGlobals {}
unsafe impl bytemuck::Zeroable for GpuGlobals {}
unsafe impl bytemuck::Pod for GpuVertex {}
unsafe impl bytemuck::Zeroable for GpuVertex {}
unsafe impl bytemuck::Pod for EdgeInstance {}
unsafe impl bytemuck::Zeroable for EdgeInstance {}
unsafe impl bytemuck::Pod for CircleInstance {}
unsafe impl bytemuck::Zeroable for CircleInstance {}
