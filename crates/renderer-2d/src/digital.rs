//! GPU overlays with a shared glyph atlas. egui only supplies ordering and clipping.
use ab_glyph::{point, Font, FontArc, ScaleFont};
use egui::{pos2, vec2, Rect};
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;
#[cfg(test)]
mod tests;

/// Text positions and sizes are relative to the overlay rectangle.
pub struct DigitalText {
    pub position: [f32; 2],
    pub size: f32,
    pub text: String,
}
pub enum DigitalSurface {
    Panel,
    Scan,
    Reconstruct,
    /// Angular frame with an inward-closing geometric seal, driven by `phase`.
    Confirmation,
    /// A one-shot recessed press and outward light pulse, driven by `phase`.
    Press,
}
/// A reusable translucent panel, optional progress track, and arbitrary text.
/// Images can be composed before/after this command through `DrawCommand::Sprite`.
pub struct DigitalDraw {
    pub surface: DigitalSurface,
    /// Set to zero to keep decorative motion still (for reduced-motion settings).
    pub seconds: f32,
    pub rect: Rect,
    pub color: [f32; 4],
    pub phase: f32,
    pub progress: Option<f32>,
    pub text: Vec<DigitalText>,
}
pub(crate) struct DigitalPainter {
    font: FontArc,
    owner: Arc<()>,
}
impl DigitalPainter {
    pub fn new(font: FontArc) -> Self {
        Self {
            font,
            owner: Arc::new(()),
        }
    }
    pub fn callback(
        &self,
        screen: Rect,
        format: wgpu::TextureFormat,
        slot: usize,
        characters: Arc<std::collections::HashSet<char>>,
        draw: DigitalDraw,
    ) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            screen,
            Callback {
                screen,
                format,
                slot,
                characters,
                draw,
                font: self.font.clone(),
                owner: self.owner.clone(),
            },
        )
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    settings: [f32; 4],
    animation: [f32; 2],
}
struct Mesh {
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}
struct Resources {
    owner: std::sync::Weak<()>,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    binding: wgpu::BindGroup,
    glyphs: HashMap<char, (Rect, f32)>,
    meshes: Vec<Mesh>,
}
const SIDE: u32 = 2048;
const CELL: u32 = 48;
impl Resources {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, owner: &Arc<()>) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Digital atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Digital glyph atlas"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Digital atlas"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Digital overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("digital.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Digital overlay"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format.is_srgb() {
                    "fs_linear"
                } else {
                    "fs_gamma"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Self {
            owner: Arc::downgrade(owner),
            pipeline,
            texture,
            binding,
            glyphs: HashMap::new(),
            meshes: Vec::new(),
        }
    }
    fn glyph(&mut self, queue: &wgpu::Queue, font: &FontArc, c: char) -> (Rect, f32) {
        if let Some(glyph) = self.glyphs.get(&c) {
            return *glyph;
        }
        let index = self.glyphs.len() as u32;
        assert!(
            index < (SIDE / CELL).pow(2),
            "digital glyph atlas capacity exceeded"
        );
        let x = index % (SIDE / CELL) * CELL;
        let y = index / (SIDE / CELL) * CELL;
        let scaled = font.as_scaled(32.);
        let id = scaled.glyph_id(c);
        let mut pixels = vec![0; (CELL * CELL) as usize];
        if let Some(outline) =
            font.outline_glyph(id.with_scale_and_position(32., point(4., 4. + scaled.ascent())))
        {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && px < CELL as i32 && py < CELL as i32 {
                    pixels[(py as u32 * CELL + px as u32) as usize] = (coverage * 255.) as u8;
                }
            });
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(CELL),
                rows_per_image: Some(CELL),
            },
            wgpu::Extent3d {
                width: CELL,
                height: CELL,
                depth_or_array_layers: 1,
            },
        );
        let glyph = (
            Rect::from_min_size(
                pos2(x as f32 / SIDE as f32, y as f32 / SIDE as f32),
                vec2(CELL as f32 / SIDE as f32, CELL as f32 / SIDE as f32),
            ),
            scaled.h_advance(id),
        );
        self.glyphs.insert(c, glyph);
        glyph
    }
}
struct Callback {
    characters: Arc<std::collections::HashSet<char>>,
    screen: Rect,
    format: wgpu::TextureFormat,
    slot: usize,
    draw: DigitalDraw,
    font: FontArc,
    owner: Arc<()>,
}
impl Callback {
    fn quad(
        &self,
        vertices: &mut Vec<Vertex>,
        rect: Rect,
        uv: Rect,
        color: [f32; 4],
        settings: [f32; 4],
    ) {
        for (p, t) in [
            (rect.left_top(), uv.left_top()),
            (rect.right_top(), uv.right_top()),
            (rect.left_bottom(), uv.left_bottom()),
            (rect.left_bottom(), uv.left_bottom()),
            (rect.right_top(), uv.right_top()),
            (rect.right_bottom(), uv.right_bottom()),
        ] {
            vertices.push(Vertex {
                position: [
                    2. * (p.x - self.screen.left()) / self.screen.width() - 1.,
                    1. - 2. * (p.y - self.screen.top()) / self.screen.height(),
                ],
                uv: [t.x, t.y],
                color,
                settings,
                animation: [self.draw.seconds, 0.],
            });
        }
    }
}
impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &egui_wgpu::ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources
            .get::<Resources>()
            .is_none_or(|r| !r.owner.ptr_eq(&Arc::downgrade(&self.owner)))
        {
            resources.insert(Resources::new(device, self.format, &self.owner));
        }
        let r = resources.get_mut::<Resources>().unwrap();
        {
            let capacity = (SIDE / CELL).pow(2) as usize;
            assert!(
                self.characters.len() <= capacity,
                "too many simultaneous digital glyphs"
            );
            let missing = self
                .characters
                .iter()
                .filter(|c| !r.glyphs.contains_key(c))
                .count();
            // Rebuild only between frames: every overlay in this frame shares the
            // same resident character set, so later callbacks cannot overwrite it.
            if r.glyphs.len() + missing > capacity {
                r.glyphs.clear();
            }
            for c in self.characters.iter() {
                r.glyph(queue, &self.font, *c);
            }
        }
        let mut vertices = Vec::new();
        let rect = if matches!(self.draw.surface, DigitalSurface::Press) {
            let t = self.draw.phase.clamp(0., 1.);
            let smooth = |a: f32, b: f32| {
                let x = ((t - a) / (b - a)).clamp(0., 1.);
                x * x * (3. - 2. * x)
            };
            let press = smooth(0.20, 0.38) * (1. - smooth(0.48, 0.64));
            self.draw
                .rect
                .shrink2(self.draw.rect.size() * 0.018 * press)
                .translate(vec2(0., self.draw.rect.height() * 0.045 * press))
        } else {
            self.draw.rect
        };
        let uv = Rect::from_min_max(pos2(0., 0.), pos2(1., 1.));
        self.quad(
            &mut vertices,
            rect,
            uv,
            self.draw.color,
            [
                match self.draw.surface {
                    DigitalSurface::Panel => 1.,
                    DigitalSurface::Scan => 3.,
                    DigitalSurface::Reconstruct => 4.,
                    DigitalSurface::Confirmation => 5.,
                    DigitalSurface::Press => 6.,
                },
                self.draw.phase,
                rect.width(),
                rect.height(),
            ],
        );
        if let Some(progress) = self.draw.progress {
            let track = Rect::from_min_max(
                rect.min + vec2(rect.width() * 0.04, rect.height() * 0.68),
                rect.max - vec2(rect.width() * 0.04, rect.height() * 0.12),
            );
            let fill = Rect::from_min_size(
                track.min,
                vec2(track.width() * progress.clamp(0., 1.), track.height()),
            );
            let mut dim = self.draw.color;
            dim[3] *= 0.15;
            self.quad(&mut vertices, track, uv, dim, [2., 1., 1., 1.]);
            let mut bright = self.draw.color;
            bright[3] *= 0.9;
            self.quad(&mut vertices, fill, uv, bright, [2., 1., 1., 1.]);
        }
        for text in &self.draw.text {
            let size = text.size * rect.height();
            let origin = rect.min
                + vec2(
                    text.position[0] * rect.width(),
                    text.position[1] * rect.height(),
                );
            let mut cursor = origin;
            for c in text.text.chars() {
                if c == '\n' {
                    cursor.x = origin.x;
                    cursor.y += size * 1.3;
                    continue;
                }
                let (uv, advance) = r.glyph(queue, &self.font, c);
                self.quad(
                    &mut vertices,
                    Rect::from_min_size(
                        cursor - vec2(size / 8., size / 8.),
                        vec2(size * 1.5, size * 1.5),
                    ),
                    uv,
                    if matches!(
                        self.draw.surface,
                        DigitalSurface::Confirmation | DigitalSurface::Press
                    ) {
                        [1., 0.87, 0.92, self.draw.color[3]]
                    } else {
                        [0.88, 0.98, 1., self.draw.color[3]]
                    },
                    [0.; 4],
                );
                cursor.x += advance * size / 32.;
            }
        }
        while r.meshes.len() <= self.slot {
            r.meshes.push(Mesh {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Digital empty mesh"),
                    size: 4,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                capacity: 0,
                count: 0,
            });
        }
        let mesh = &mut r.meshes[self.slot];
        let bytes = bytemuck::cast_slice(&vertices);
        if mesh.capacity < bytes.len() {
            mesh.buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Digital mesh"),
                contents: bytes,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            mesh.capacity = bytes.len();
        } else {
            queue.write_buffer(&mesh.buffer, 0, bytes);
        }
        mesh.count = vertices.len() as u32;
        Vec::new()
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let r = resources.get::<Resources>().unwrap();
        let mesh = &r.meshes[self.slot];
        pass.set_pipeline(&r.pipeline);
        pass.set_bind_group(0, &r.binding, &[]);
        pass.set_vertex_buffer(0, mesh.buffer.slice(..));
        pass.draw(0..mesh.count, 0..1);
    }
}
