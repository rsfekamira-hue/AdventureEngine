//! Stateless GPU particles: one compute invocation and one quad per particle/trail.
//! CPU prepares immutable glyph atlases once and updates one uniform per emitter.
use ab_glyph::{point, Font, FontArc, ScaleFont};
use egui::Rect;
use engine_core::particles::{ParticleEmitter, ParticleShape};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use wgpu::util::DeviceExt;

pub struct ParticleDraw {
    pub emitters: Arc<Vec<ParticleEmitter>>,
    pub rect: Rect,
    pub milliseconds: f32,
    pub fade: f32,
    pub tint: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    spawn: [f32; 4],
    motion: [f32; 4],
    acceleration_size: [f32; 4],
    destination: [f32; 4],
    color: [f32; 4],
    settings: [f32; 4],
    ids: [u32; 4],
    rect: [f32; 4],
    screen: [f32; 4],
    clock: [f32; 4],
    tint: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Glyph {
    uv: [f32; 4],
    metrics: [f32; 4],
}
struct Atlas {
    size: [u32; 2],
    pixels: Vec<u8>,
    glyphs: Vec<Glyph>,
}

fn atlas(font: Option<&FontArc>, shape: &ParticleShape) -> Atlas {
    let texts: Vec<String> = match shape {
        ParticleShape::Quad => {
            return Atlas {
                size: [1, 1],
                pixels: vec![255],
                glyphs: vec![Glyph {
                    uv: [0., 0., 1., 1.],
                    metrics: [1., 0., 0., 0.],
                }],
            }
        }
        ParticleShape::Text { characters, .. } => {
            characters.chars().map(|c| c.to_string()).collect()
        }
        ParticleShape::Phrases { phrases, .. } => phrases.clone(),
    };
    let font = font.expect("text particles require the renderer font");
    let scaled = font.as_scaled(32.0);
    let widths: Vec<_> = texts
        .iter()
        .map(|s| {
            (s.chars()
                .map(|c| scaled.h_advance(scaled.glyph_id(c)))
                .sum::<f32>()
                .ceil() as u32
                + 8)
            .max(8)
        })
        .collect();
    let cell_width = *widths.iter().max().unwrap();
    let columns = (1024 / cell_width).max(1);
    let height = 48;
    let size = [
        columns * cell_width,
        (texts.len() as u32).div_ceil(columns) * height,
    ];
    let mut result = Atlas {
        size,
        pixels: vec![0; (size[0] * size[1]) as usize],
        glyphs: Vec::new(),
    };
    for (index, text) in texts.iter().enumerate() {
        let x = index as u32 % columns * cell_width;
        let y = index as u32 / columns * height;
        let mut cursor = 4.0;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            let glyph = id.with_scale_and_position(32.0, point(cursor, 4.0 + scaled.ascent()));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px >= 0 && px < widths[index] as i32 && py >= 0 && py < height as i32 {
                        let slot = ((y + py as u32) * size[0] + x + px as u32) as usize;
                        result.pixels[slot] = result.pixels[slot].max((coverage * 255.0) as u8);
                    }
                });
            }
            cursor += scaled.h_advance(id);
        }
        result.glyphs.push(Glyph {
            uv: [
                x as f32 / size[0] as f32,
                y as f32 / size[1] as f32,
                widths[index] as f32 / size[0] as f32,
                height as f32 / size[1] as f32,
            ],
            metrics: [widths[index] as f32 / height as f32, 0., 0., 0.],
        });
    }
    result
}

#[derive(Default)]
pub(crate) struct ParticlePainter {
    font: Option<FontArc>,
    atlases: HashMap<usize, (Weak<Vec<ParticleEmitter>>, Vec<Arc<Atlas>>)>,
}
impl ParticlePainter {
    pub fn new(font: FontArc) -> Self {
        Self {
            font: Some(font),
            ..Self::default()
        }
    }
    pub fn callbacks(
        &mut self,
        screen: Rect,
        format: wgpu::TextureFormat,
        draw: ParticleDraw,
    ) -> Vec<egui::PaintCallback> {
        self.atlases
            .retain(|_, (owner, _)| owner.strong_count() > 0);
        let key = Arc::as_ptr(&draw.emitters) as usize;
        let (_, atlases) = self.atlases.entry(key).or_insert_with(|| {
            (
                Arc::downgrade(&draw.emitters),
                draw.emitters
                    .iter()
                    .map(|e| Arc::new(atlas(self.font.as_ref(), &e.shape)))
                    .collect(),
            )
        });
        draw.emitters
            .iter()
            .enumerate()
            .map(|(index, e)| {
                let change = match e.shape {
                    ParticleShape::Text { change_ms, .. }
                    | ParticleShape::Phrases { change_ms, .. } => change_ms as f32,
                    _ => 1.,
                };
                let params = Params {
                    spawn: e.spawn,
                    motion: [e.velocity[0], e.velocity[1], e.spread[0], e.spread[1]],
                    acceleration_size: [e.acceleration[0], e.acceleration[1], e.size[0], e.size[1]],
                    destination: e.destination.unwrap_or([0., 0., -1., -1.]),
                    color: e.color,
                    settings: [e.lifetime_ms as f32, e.glow, e.glitch, change],
                    ids: [
                        e.seed,
                        u32::from(e.count),
                        u32::from(e.trails),
                        atlases[index].glyphs.len() as u32,
                    ],
                    rect: [
                        draw.rect.min.x,
                        draw.rect.min.y,
                        draw.rect.width(),
                        draw.rect.height(),
                    ],
                    screen: [screen.min.x, screen.min.y, screen.width(), screen.height()],
                    clock: [
                        draw.milliseconds,
                        draw.fade,
                        e.trail_ms as f32,
                        if matches!(e.shape, ParticleShape::Quad) {
                            0.
                        } else {
                            1.
                        },
                    ],
                    tint: draw.tint,
                };
                egui_wgpu::Callback::new_paint_callback(
                    screen,
                    ParticleCallback {
                        owner: draw.emitters.clone(),
                        index,
                        atlas: atlases[index].clone(),
                        params,
                        format,
                    },
                )
            })
            .collect()
    }
}

struct Prepared {
    owner: Weak<Vec<ParticleEmitter>>,
    uniform: wgpu::Buffer,
    particles: wgpu::Buffer,
    compute: wgpu::BindGroup,
    render: wgpu::BindGroup,
}
pub(crate) struct Resources {
    compute: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    compute_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    groups: HashMap<(usize, usize), Prepared>,
}
impl Resources {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let buffer = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Particle compute layout"),
            entries: &[
                buffer(
                    0,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::BufferBindingType::Uniform,
                ),
                buffer(
                    1,
                    wgpu::ShaderStages::COMPUTE,
                    wgpu::BufferBindingType::Storage { read_only: false },
                ),
            ],
        });
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Particle render layout"),
            entries: &[
                buffer(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BufferBindingType::Uniform,
                ),
                buffer(
                    1,
                    wgpu::ShaderStages::VERTEX,
                    wgpu::BufferBindingType::Uniform,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Particle simulation"),
            source: wgpu::ShaderSource::Wgsl(include_str!("particles_compute.wgsl").into()),
        });
        let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Particle simulation"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&compute_layout],
                    push_constant_ranges: &[],
                }),
            ),
            module: &compute_shader,
            entry_point: Some("simulate"),
            compilation_options: Default::default(),
            cache: None,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Particle quads"),
            source: wgpu::ShaderSource::Wgsl(include_str!("particles_render.wgsl").into()),
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4];
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Particle quads"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&render_layout],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 48,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format.is_srgb() { "linear" } else { "gamma" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Self {
            compute,
            render,
            compute_layout,
            render_layout,
            groups: HashMap::new(),
        }
    }
    fn prepare_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        callback: &ParticleCallback,
    ) -> Prepared {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Particle parameters"),
            contents: bytemuck::bytes_of(&callback.params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let particles = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle instances"),
            size: u64::from(callback.instances()) * 48,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut glyph_table = [Glyph {
            uv: [0.; 4],
            metrics: [0.; 4],
        }; 128];
        glyph_table[..callback.atlas.glyphs.len()].copy_from_slice(&callback.atlas.glyphs);
        let glyphs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Particle glyph metrics"),
            contents: bytemuck::cast_slice(&glyph_table),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Particle glyph atlas"),
            size: wgpu::Extent3d {
                width: callback.atlas.size[0],
                height: callback.atlas.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &callback.atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(callback.atlas.size[0]),
                rows_per_image: None,
            },
            texture.size(),
        );
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let compute = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Particle compute bindings"),
            layout: &self.compute_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: particles.as_entire_binding(),
                },
            ],
        });
        let render = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Particle render bindings"),
            layout: &self.render_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: glyphs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Prepared {
            owner: Arc::downgrade(&callback.owner),
            uniform,
            particles,
            compute,
            render,
        }
    }
}
struct ParticleCallback {
    owner: Arc<Vec<ParticleEmitter>>,
    index: usize,
    atlas: Arc<Atlas>,
    params: Params,
    format: wgpu::TextureFormat,
}
impl ParticleCallback {
    fn key(&self) -> (usize, usize) {
        (Arc::as_ptr(&self.owner) as usize, self.index)
    }
    fn instances(&self) -> u32 {
        self.params.ids[1] * (self.params.ids[2] + 1)
    }
}
impl egui_wgpu::CallbackTrait for ParticleCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<Resources>().is_none() {
            resources.insert(Resources::new(device, self.format));
        }
        let resources = resources
            .get_mut::<Resources>()
            .expect("GPU particle backend installed");
        resources.groups.retain(|_, g| g.owner.strong_count() > 0);
        if !resources.groups.contains_key(&self.key()) {
            let group = resources.prepare_group(device, queue, self);
            resources.groups.insert(self.key(), group);
        }
        let group = &resources.groups[&self.key()];
        queue.write_buffer(&group.uniform, 0, bytemuck::bytes_of(&self.params));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Particles"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&resources.compute);
        pass.set_bind_group(0, &group.compute, &[]);
        pass.dispatch_workgroups(self.instances().div_ceil(64), 1, 1);
        Vec::new()
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let resources = resources
            .get::<Resources>()
            .expect("GPU particle backend installed");
        let group = &resources.groups[&self.key()];
        pass.set_pipeline(&resources.render);
        pass.set_bind_group(0, &group.render, &[]);
        pass.set_vertex_buffer(0, group.particles.slice(..));
        pass.draw(0..6, 0..self.instances());
    }
}

#[cfg(test)]
mod tests;
