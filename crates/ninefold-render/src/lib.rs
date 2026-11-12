//! A window with the world in it.
//!
//! Four things get drawn, in this order, and the order is the design:
//!
//! 1. **The world.** One textured, lit, fogged pipeline, one draw call per chunk column.
//! 2. **Entities.** The same pipeline and the same vertex format, with the mob sheet bound in
//!    place of the block atlas — which is the whole reason the vertex format is shared.
//! 3. **Lines.** Hitboxes and the outline round the block you are looking at.
//! 4. **The interface.** Screen-space quads, one pass, with the texture swapped between the
//!    widget sheets, the item sheet, the block atlas (for the blocks drawn in the hotbar) and
//!    the font.
//!
//! Nothing here knows what a block is, what an item is or what a menu looks like. It is handed
//! triangles and told which picture to put on them.

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
    /// What to multiply the texel by. Grass and leaves ship greyscale and are coloured by the
    /// biome they are standing in, so this cannot be baked into the atlas.
    pub tint: [f32; 3],
    /// How lit this corner is, 0..1, straight from the server's light data.
    pub light: f32,
}

impl Vertex {
    /// A vertex with no tint, which is almost all of them.
    #[must_use]
    pub const fn plain(position: [f32; 3], normal: [f32; 3], uv: [f32; 2], light: f32) -> Self {
        Self {
            position,
            normal,
            uv,
            tint: [1.0, 1.0, 1.0],
            light,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_projection: Mat4,
    /// Where the sun is, for the flat shading.
    sun: [f32; 4],
    /// Where the camera is, and how far it can see, for the fog.
    eye: [f32; 4],
    /// The colour distance fades into — the same as the sky, or the join shows.
    sky: [f32; 4],
    /// x: how bright the daylight is, 0..1. y: how far the world is drawn.
    world: [f32; 4],
}

const SHADER: &str = r"
struct Uniforms {
    view_projection: mat4x4<f32>,
    sun: vec4<f32>,
    eye: vec4<f32>,
    sky: vec4<f32>,
    world: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
    @location(2) distance: f32,
    @location(3) tint: vec3<f32>,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec3<f32>,
    @location(4) light: f32,
) -> Out {
    var out: Out;
    out.clip = u.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.tint = tint;
    // Two things darken a face: which way it points, and how much light reaches it. The first
    // is only for shape — without it every side of a cube looks identical.
    let facing = max(dot(normalize(normal), normalize(u.sun.xyz)), 0.0);
    let shape = 0.72 + 0.28 * facing;
    // Never fully black: caves should be gloomy, not invisible. Night dims everything, with a
    // floor of its own so the world stays readable by moonlight.
    let reached = 0.12 + 0.88 * light;
    out.shade = shape * reached * max(u.world.x, 0.25);
    // The w of a perspective-projected point is its distance along the view axis, so this needs
    // no camera position and cannot be wrong about where the eye is.
    out.distance = out.clip.w;
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
    // Fade into the sky over the last quarter of the view, so chunks arriving at the edge of
    // the world appear out of haze instead of popping into existence.
    let far = u.world.y;
    let fog = clamp((in.distance - far * 0.72) / (far * 0.28), 0.0, 1.0);
    let lit = texel.rgb * in.tint * in.shade;
    return vec4<f32>(mix(lit, u.sky.rgb, fog), 1.0);
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
    eye: vec4<f32>,
    sky: vec4<f32>,
    world: vec4<f32>,
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

// On the glass: anything already in clip space.
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

/// A character of text or a piece of interface art, in clip space.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Glyph {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub colour: [f32; 4],
}

/// Text and interface art, drawn from Minecraft's own sheets.
///
/// `ascii.png` is a 16x16 grid of glyphs, white with an alpha cut-out, so the colour comes from
/// the vertex; the widget sheets are already coloured, so the vertex colour is left white and
/// multiplies through to no effect. One shader does both.
const TEXT_SHADER: &str = r"
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
};
@group(0) @binding(1) var font: texture_2d<f32>;
@group(0) @binding(2) var font_sampler: sampler;

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
) -> Out {
    var out: Out;
    out.clip = vec4<f32>(position, 1.0);
    out.uv = uv;
    out.colour = colour;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(font, font_sampler, in.uv);
    if texel.a < 0.02 {
        discard;
    }
    return vec4<f32>(texel.rgb * in.colour.rgb, texel.a * in.colour.a);
}
";

/// How far the world is drawn. The fog has to end before this, or chunks pop in at full
/// brightness right on the edge.
const FAR: f32 = 320.0;

/// 1.8's daytime sky, and so the colour the fog fades into.
const SKY: [f32; 3] = [0.47, 0.65, 1.0];

/// One set of buffers with a count of indices.
type Indexed = (wgpu::Buffer, wgpu::Buffer, u32);

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
    columns: std::collections::HashMap<(i32, i32), Indexed>,
    outline: wgpu::RenderPipeline,
    highlight: Option<(wgpu::Buffer, u32)>,
    boxes: Option<(wgpu::Buffer, u32)>,
    /// Mobs and players: the world pipeline with the mob sheet bound.
    mobs: Option<wgpu::BindGroup>,
    mob_mesh: Option<Indexed>,
    text: wgpu::RenderPipeline,
    font: Option<wgpu::BindGroup>,
    text_mesh: Option<(wgpu::Buffer, u32)>,
    gui: Option<wgpu::BindGroup>,
    gui_mesh: Option<(wgpu::Buffer, u32)>,
    items: Option<wgpu::BindGroup>,
    item_mesh: Option<(wgpu::Buffer, u32)>,
    /// Interface quads that sample the block atlas: the blocks drawn in the hotbar.
    block_icons: Option<(wgpu::Buffer, u32)>,
    sky: [f32; 3],
    daylight: f32,
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
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3, 4 => Float32],
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
        let overlay_pipeline = |entry: &str, topology, depth: bool| {
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

        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER.into()),
        });
        let text = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Glyph>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let depth = depth_view(&device, &config);

        Ok(Self {
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
            highlight: None,
            boxes: None,
            mobs: None,
            mob_mesh: None,
            text,
            font: None,
            text_mesh: None,
            gui: None,
            gui_mesh: None,
            items: None,
            item_mesh: None,
            block_icons: None,
            sky: SKY,
            daylight: 1.0,
        })
    }

    /// Put pixels on the GPU and hand back something to sample from.
    fn upload_texture(
        &self,
        label: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// One texture plus the shared uniforms, which is the only shape of bind group here.
    fn bind(&self, label: &str, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Hand over the texture atlas. Everything drawn afterwards samples from it.
    pub fn set_atlas(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let view = self.upload_texture("atlas", width, height, rgba);
        self.bind_group = Some(self.bind("world", &view));
    }

    /// Hand over the sheet the mob and player models are drawn from.
    pub fn set_mob_sheet(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let view = self.upload_texture("mobs", width, height, rgba);
        self.mobs = Some(self.bind("mobs", &view));
    }

    /// Hand over the font sheet. Text can only be drawn once this has been called.
    pub fn set_font(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let view = self.upload_texture("font", width, height, rgba);
        self.font = Some(self.bind("font", &view));
    }

    /// Hand over the interface sheets — the hotbar, hearts, menu art and so on.
    pub fn set_gui_sheets(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let view = self.upload_texture("gui", width, height, rgba);
        self.gui = Some(self.bind("gui", &view));
    }

    /// Hand over the sheet of item pictures.
    pub fn set_item_sheet(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let view = self.upload_texture("items", width, height, rgba);
        self.items = Some(self.bind("items", &view));
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = depth_view(&self.device, &self.config);
    }

    #[must_use]
    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }

    /// What colour the sky is, and how bright the day is. Night is both darker and bluer.
    pub fn set_daylight(&mut self, brightness: f32) {
        self.daylight = brightness.clamp(0.0, 1.0);
        // Night is not black: 1.8's night sky is a very dark blue, and the fog has to match it
        // or the horizon shows a seam.
        let night = [0.02, 0.03, 0.09];
        let mix = self.daylight.powf(1.5);
        self.sky = [
            night[0] + (SKY[0] - night[0]) * mix,
            night[1] + (SKY[1] - night[1]) * mix,
            night[2] + (SKY[2] - night[2]) * mix,
        ];
    }

    /// Give one column its geometry, replacing whatever it had.
    pub fn set_column(&mut self, at: (i32, i32), vertices: &[Vertex], indices: &[u32]) {
        match indexed(&self.device, "column", vertices, indices) {
            Some(buffers) => {
                self.columns.insert(at, buffers);
            }
            None => {
                self.columns.remove(&at);
            }
        }
    }

    /// Every mob and player in one mesh, rebuilt each frame. There are rarely more than a few
    /// dozen on screen, and they all move, so a per-entity buffer would buy nothing.
    pub fn set_mobs(&mut self, vertices: &[Vertex], indices: &[u32]) {
        self.mob_mesh = indexed(&self.device, "mobs", vertices, indices);
    }

    /// Outline the block being looked at, or nothing.
    pub fn set_highlight(&mut self, block: Option<(i32, i32, i32)>) {
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
        self.highlight = vertex_buffer(&self.device, "highlight", &lines);
    }

    /// The interface quads for this frame, already in clip space.
    pub fn set_sprites(&mut self, quads: &[Glyph]) {
        self.gui_mesh = vertex_buffer(&self.device, "gui", quads);
    }

    /// Interface quads that sample the item sheet.
    pub fn set_item_quads(&mut self, quads: &[Glyph]) {
        self.item_mesh = vertex_buffer(&self.device, "items", quads);
    }

    /// Interface quads that sample the block atlas, for the blocks drawn in a slot.
    pub fn set_block_icons(&mut self, quads: &[Glyph]) {
        self.block_icons = vertex_buffer(&self.device, "block icons", quads);
    }

    /// The text for this frame, already laid out and in clip space.
    pub fn set_text_quads(&mut self, quads: &[Glyph]) {
        self.text_mesh = vertex_buffer(&self.device, "text", quads);
    }

    /// Draw a box round each thing in the world — what F3+B shows.
    pub fn set_boxes(&mut self, boxes: &[([f32; 3], [f32; 3], [f32; 4])]) {
        if boxes.is_empty() {
            self.boxes = None;
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
        self.boxes = vertex_buffer(&self.device, "boxes", &lines);
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
        let projection = camera::perspective(70.0, self.aspect(), 0.05, FAR);
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                view_projection: camera::multiply(view, projection),
                // Afternoon sun, slightly off to one side so the faces of a cube differ.
                sun: [0.4, 0.9, 0.25, 0.0],
                eye: [eye[0], eye[1], eye[2], FAR],
                sky: [self.sky[0], self.sky[1], self.sky[2], 1.0],
                world: [self.daylight, FAR, 0.0, 0.0],
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
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(self.sky[0]),
                            g: f64::from(self.sky[1]),
                            b: f64::from(self.sky[2]),
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
            }
            // Everything alive, out of its own sheet, through the same pipeline.
            if let (Some((vertices, indices, count)), Some(mobs)) = (&self.mob_mesh, &self.mobs) {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, Some(mobs), &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
            if let Some(bind_group) = &self.bind_group {
                pass.set_bind_group(0, Some(bind_group), &[]);
                for (buffer, count) in [&self.boxes, &self.highlight].into_iter().flatten() {
                    pass.set_pipeline(&self.outline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
            // The interface, in layers, each with its own picture bound.
            pass.set_pipeline(&self.text);
            for (mesh, group) in [
                (&self.gui_mesh, &self.gui),
                (&self.block_icons, &self.bind_group),
                (&self.item_mesh, &self.items),
                // Text last, over everything.
                (&self.text_mesh, &self.font),
            ] {
                if let (Some((buffer, count)), Some(group)) = (mesh, group) {
                    pass.set_bind_group(0, Some(group), &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        // Presenting moved onto the queue in wgpu 30.
        self.queue.present(frame);
    }
}

/// A vertex buffer, or `None` for nothing to draw.
fn vertex_buffer<T: Pod>(
    device: &wgpu::Device,
    label: &str,
    data: &[T],
) -> Option<(wgpu::Buffer, u32)> {
    use wgpu::util::DeviceExt;
    if data.is_empty() {
        return None;
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::VERTEX,
    });
    Some((buffer, u32::try_from(data.len()).unwrap_or(u32::MAX)))
}

/// Vertices and indices together, or `None` for nothing to draw.
fn indexed(
    device: &wgpu::Device,
    label: &str,
    vertices: &[Vertex],
    indices: &[u32],
) -> Option<Indexed> {
    use wgpu::util::DeviceExt;
    if indices.is_empty() {
        return None;
    }
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Some((
        vertex_buffer,
        index_buffer,
        u32::try_from(indices.len()).unwrap_or(u32::MAX),
    ))
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
