//! A window with the world in it.
//!
//! Deliberately the small version: one pipeline, flat-shaded triangles, a camera that sits where
//! the player is. No texture atlas, no lighting model, no transparency pass. Those are worth
//! having and none of them can be judged until something is on screen at all, so this is the
//! thing that gets there.

pub mod camera;

use bytemuck::{Pod, Zeroable};
use camera::Mat4;

/// A vertex as the GPU sees it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Where in the atlas, already in 0..1 atlas space.
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_projection: Mat4,
    /// Where the sun is, for the flat shading.
    sun: [f32; 4],
}

const SHADER: &str = r"
struct Uniforms {
    view_projection: mat4x4<f32>,
    sun: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> Out {
    var out: Out;
    out.clip = u.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    // Flat directional light with a floor, so faces away from the sun are shaded, not black.
    let lit = max(dot(normalize(normal), normalize(u.sun.xyz)), 0.0);
    out.shade = 0.45 + 0.55 * lit;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    // Cutout, not blending: leaves and the like are either there or they are not, and this
    // keeps them working without sorting anything back to front.
    if texel.a < 0.5 {
        discard;
    }
    return vec4<f32>(texel.rgb * in.shade, 1.0);
}
";

/// A line or a flat shape drawn over the world.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Overlay {
    pub position: [f32; 3],
    pub colour: [f32; 4],
}

/// The crosshair and the outline round the block you are looking at.
const OVERLAY_SHADER: &str = r"
struct Uniforms {
    view_projection: mat4x4<f32>,
    sun: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

// In the world: the outline round a block.
@vertex
fn vs_world(@location(0) position: vec3<f32>, @location(1) colour: vec4<f32>) -> Out {
    var out: Out;
    out.clip = u.view_projection * vec4<f32>(position, 1.0);
    out.colour = colour;
    return out;
}

// On the glass: the crosshair, already in clip space.
@vertex
fn vs_screen(@location(0) position: vec3<f32>, @location(1) colour: vec4<f32>) -> Out {
    var out: Out;
    out.clip = vec4<f32>(position, 1.0);
    out.colour = colour;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return in.colour;
}
";

/// Everything needed to draw, tied to one window.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Built once an atlas has been handed over; nothing can be drawn before that.
    bind_group: Option<wgpu::BindGroup>,
    depth: wgpu::TextureView,
    /// One set of buffers per chunk column, so a column that changes costs only itself.
    /// Rebuilding the lot for one new chunk is what made this stutter.
    columns: std::collections::HashMap<(i32, i32), (wgpu::Buffer, wgpu::Buffer, u32)>,
    outline: wgpu::RenderPipeline,
    crosshair: wgpu::RenderPipeline,
    highlight: Option<(wgpu::Buffer, u32)>,
    entities: Option<(wgpu::Buffer, u32)>,
    crosshair_mesh: Option<(wgpu::Buffer, u32)>,
}

impl Renderer {
    /// Set up a device and a swapchain for a window that is already open.
    ///
    /// # Errors
    /// If no GPU adapter will take the surface, or the device cannot be created.
    pub fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(target)
            .map_err(|e| format!("no surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ninefold"),
            ..Default::default()
        }))
        .map_err(|e| format!("no device: {e}"))?;

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| "this GPU cannot draw to this window".to_owned())?;
        // Prefer an sRGB target so colours are not washed out.
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(srgb) = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
        {
            config.format = srgb;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        let format = config.format;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Nearest filtering: Minecraft's textures are 16 pixels across and are meant to look it.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(format.into())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                // The mesher winds faces anticlockwise seen from outside, so the inside of a
                // block is never drawn.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // Two more pipelines sharing one shader: lines in the world, and flat shapes on the
        // glass. Both reuse the same bind group; the overlay shader simply ignores the atlas.
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let overlay_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<Overlay>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        })];
        let mut overlay_pipeline = |entry: &str, topology, depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("overlay"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &overlay_shader,
                    entry_point: Some(entry),
                    buffers: &overlay_buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &overlay_shader,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    // The outline sits exactly on the block's surface, so it has to pass where
                    // the depths are equal or it z-fights with the face it outlines.
                    depth_compare: Some(if depth {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let outline = overlay_pipeline("vs_world", wgpu::PrimitiveTopology::LineList, true);
        let crosshair = overlay_pipeline("vs_screen", wgpu::PrimitiveTopology::TriangleList, false);

        let depth = depth_view(&device, &config);

        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            uniforms,
            layout,
            sampler,
            bind_group: None,
            depth,
            columns: std::collections::HashMap::new(),
            outline,
            crosshair,
            highlight: None,
            entities: None,
            crosshair_mesh: None,
        };
        renderer.build_crosshair();
        Ok(renderer)
    }

    /// Hand over the texture atlas. Everything drawn afterwards samples from it.
    pub fn set_atlas(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = depth_view(&self.device, &self.config);
        self.build_crosshair();
    }

    #[must_use]
    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }

    /// Give one column its geometry, replacing whatever it had.
    pub fn set_column(&mut self, at: (i32, i32), vertices: &[Vertex], indices: &[u32]) {
        use wgpu::util::DeviceExt;
        if indices.is_empty() {
            self.columns.remove(&at);
            return;
        }
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("column vertices"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("column indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.columns.insert(
            at,
            (
                vertex_buffer,
                index_buffer,
                u32::try_from(indices.len()).unwrap_or(u32::MAX),
            ),
        );
    }

    /// Outline the block being looked at, or nothing.
    pub fn set_highlight(&mut self, block: Option<(i32, i32, i32)>) {
        use wgpu::util::DeviceExt;
        let Some((x, y, z)) = block else {
            self.highlight = None;
            return;
        };
        // A hair outside the block, so the lines are not buried in its own faces.
        const OUT: f32 = 0.002;
        let (lo, hi) = (-OUT, 1.0 + OUT);
        let corner = |cx: f32, cy: f32, cz: f32| Overlay {
            position: [x as f32 + cx, y as f32 + cy, z as f32 + cz],
            colour: [0.0, 0.0, 0.0, 0.65],
        };
        let mut lines = Vec::with_capacity(24);
        // Four uprights, and the squares top and bottom.
        for (ax, az) in [(lo, lo), (hi, lo), (hi, hi), (lo, hi)] {
            lines.push(corner(ax, lo, az));
            lines.push(corner(ax, hi, az));
        }
        for level in [lo, hi] {
            let square = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)];
            for i in 0..4 {
                let (ax, az) = square[i];
                let (bx, bz) = square[(i + 1) % 4];
                lines.push(corner(ax, level, az));
                lines.push(corner(bx, level, bz));
            }
        }
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("highlight"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.highlight = Some((buffer, u32::try_from(lines.len()).unwrap_or(0)));
    }

    /// Draw a box round each thing in the world. Crude, but it is the difference between a
    /// world with other players in it and an empty one; proper models come later.
    pub fn set_entities(&mut self, boxes: &[([f32; 3], [f32; 3], [f32; 4])]) {
        use wgpu::util::DeviceExt;
        if boxes.is_empty() {
            self.entities = None;
            return;
        }
        let mut lines = Vec::with_capacity(boxes.len() * 24);
        for (centre, size, colour) in boxes {
            let (hw, h) = (size[0] / 2.0, size[1]);
            let corner = |dx: f32, dy: f32, dz: f32| Overlay {
                position: [centre[0] + dx, centre[1] + dy, centre[2] + dz],
                colour: *colour,
            };
            let feet = [(-hw, -hw), (hw, -hw), (hw, hw), (-hw, hw)];
            for (x, z) in feet {
                lines.push(corner(x, 0.0, z));
                lines.push(corner(x, h, z));
            }
            for level in [0.0, h] {
                for i in 0..4 {
                    let (ax, az) = feet[i];
                    let (bx, bz) = feet[(i + 1) % 4];
                    lines.push(corner(ax, level, az));
                    lines.push(corner(bx, level, bz));
                }
            }
        }
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("entities"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.entities = Some((buffer, u32::try_from(lines.len()).unwrap_or(0)));
    }

    /// Rebuild the crosshair for the current window shape, so it stays square.
    fn build_crosshair(&mut self) {
        use wgpu::util::DeviceExt;
        let aspect = self.aspect();
        // Nine pixels long, one wide, in clip space.
        let long = 9.0 / self.config.height as f32;
        let thin = 1.0 / self.config.height as f32;
        let colour = [1.0, 1.0, 1.0, 0.8];
        let quad = |w: f32, h: f32| {
            let corners = [[-w, -h], [w, -h], [w, h], [-w, -h], [w, h], [-w, h]];
            corners.map(|[x, y]| Overlay {
                position: [x, y, 0.0],
                colour,
            })
        };
        let mut shape = Vec::with_capacity(12);
        shape.extend(quad(long / aspect, thin));
        shape.extend(quad(thin / aspect, long));
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("crosshair"),
                contents: bytemuck::cast_slice(&shape),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.crosshair_mesh = Some((buffer, u32::try_from(shape.len()).unwrap_or(0)));
    }

    /// Forget a column the server has unloaded.
    pub fn remove_column(&mut self, at: (i32, i32)) {
        self.columns.remove(&at);
    }

    #[must_use]
    pub fn columns_drawn(&self) -> usize {
        self.columns.len()
    }

    /// Draw one frame from the given eye.
    pub fn draw(&mut self, eye: [f32; 3], direction: [f32; 3]) {
        let view = camera::look_to(eye, direction, [0.0, 1.0, 0.0]);
        let projection = camera::perspective(70.0, self.aspect(), 0.05, 512.0);
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                view_projection: camera::multiply(view, projection),
                // Afternoon sun, slightly off to one side so the faces of a cube differ.
                sun: [0.4, 0.9, 0.25, 0.0],
            }),
        );

        // Suboptimal still draws; the rest mean there is no frame to draw into this time.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => {
                // Lost, outdated, timed out or hidden: reconfigure and come back next frame.
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // 1.8's sky blue.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.47,
                            g: 0.65,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(bind_group) = &self.bind_group {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, Some(bind_group), &[]);
                // One draw per column. A couple of hundred draws costs far less than rebuilding
                // every buffer because one chunk arrived.
                for (vertices, indices, count) in self.columns.values() {
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*count, 0, 0..1);
                }

                if let Some((lines, count)) = &self.entities {
                    pass.set_pipeline(&self.outline);
                    pass.set_vertex_buffer(0, lines.slice(..));
                    pass.draw(0..*count, 0..1);
                }
                if let Some((lines, count)) = &self.highlight {
                    pass.set_pipeline(&self.outline);
                    pass.set_vertex_buffer(0, lines.slice(..));
                    pass.draw(0..*count, 0..1);
                }
                if let Some((shape, count)) = &self.crosshair_mesh {
                    pass.set_pipeline(&self.crosshair);
                    pass.set_vertex_buffer(0, shape.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        // Presenting moved onto the queue in wgpu 30.
        self.queue.present(frame);
    }
}

fn depth_view(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
