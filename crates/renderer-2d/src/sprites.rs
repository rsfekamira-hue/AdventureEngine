//! Direct wgpu sprite draws. egui only schedules their ordering and scissor.
use egui::{Color32, Rect};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Weak,
    },
};
use wgpu::util::DeviceExt;

#[cfg(test)]
mod tests;

// Stored in egui's callback resource slot because wgpu handles on Web are not
// Send + Sync. Callbacks carry only geometry and IDs, on both Web and native.
pub(super) struct Resources {
    pipeline: wgpu::RenderPipeline,
    draws: HashMap<u64, (Weak<Draw>, wgpu::Buffer)>,
}

pub(super) fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Resources {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Direct sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprites.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite pipeline layout"),
            bind_group_layouts: &[layout],
            push_constant_ranges: &[],
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Direct sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
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
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            draws: HashMap::new(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
}
struct Draw {
    id: u64,
    vertices: [Vertex; 4],
}
struct SpriteCallback {
    image: u64,
    draw: Arc<Draw>,
}

pub(super) fn callback(
    screen: Rect,
    image: u64,
    rect: Rect,
    uv: Rect,
    rotation: f32,
    tint: Color32,
) -> egui::PaintCallback {
    let (sin, cos) = rotation.sin_cos();
    let center = rect.center();
    let tint = tint.to_array().map(|channel| channel as f32 / 255.0);
    let vertices = [
        (rect.left_top(), uv.left_top()),
        (rect.right_top(), uv.right_top()),
        (rect.left_bottom(), uv.left_bottom()),
        (rect.right_bottom(), uv.right_bottom()),
    ]
    .map(|(point, uv)| {
        let local = point - center;
        let point =
            center + egui::vec2(local.x * cos - local.y * sin, local.x * sin + local.y * cos);
        Vertex {
            position: [
                2.0 * (point.x - screen.min.x) / screen.width() - 1.0,
                1.0 - 2.0 * (point.y - screen.min.y) / screen.height(),
            ],
            uv: [uv.x, uv.y],
            tint,
        }
    });
    egui_wgpu::Callback::new_paint_callback(
        screen,
        SpriteCallback {
            image,
            draw: Arc::new(Draw {
                id: next_id(),
                vertices,
            }),
        },
    )
}

// This trait is solely the callback required by egui-wgpu. The game's image
// messages, ownership and backend selection do not use trait objects.
impl egui_wgpu::CallbackTrait for SpriteCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let resources = resources
            .get_mut::<Resources>()
            .expect("sprite backend installed");
        // Frozen loading screens reuse the same callback/buffer. Old frame
        // geometry is reclaimed; this cache never retains image data.
        resources
            .draws
            .retain(|_, (draw, _)| draw.strong_count() != 0);
        resources.draws.entry(self.draw.id).or_insert_with(|| {
            (
                Arc::downgrade(&self.draw),
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Sprite vertices"),
                    contents: bytemuck::cast_slice(&self.draw.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            )
        });
        Vec::new()
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let resources = callback_resources
            .get::<Resources>()
            .expect("sprite backend installed");
        let images = callback_resources
            .get::<crate::textures::gpu::ImageResources>()
            .expect("image store installed");
        let Some(image) = images.images.get(&self.image) else {
            return;
        };
        let Some((_, vertices)) = resources.draws.get(&self.draw.id) else {
            return;
        };
        pass.set_pipeline(&resources.pipeline);
        pass.set_bind_group(0, &image.binding, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..4, 0..1);
    }
}
