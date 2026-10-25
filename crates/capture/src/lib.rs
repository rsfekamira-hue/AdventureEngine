//! Offscreen egui rendering and PNG readback, independent of application state.
use std::path::Path;

pub struct Capture {
    ctx: egui::Context,
    gpu: egui_wgpu::Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
}
impl Capture {
    /// The caller registers fonts/textures and supplies its normal UI through save.
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("capture dimensions must be nonzero".into());
        }
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: None,
                    ..Default::default()
                })
                .await
                .ok_or("撮影用GPUがありません")?;
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Scene capture"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_defaults()
                            .using_resolution(adapter.limits()),
                        memory_hints: Default::default(),
                    },
                    None,
                )
                .await
                .map_err(|e| e.to_string())?;

            let max = device.limits().max_texture_dimension_2d;
            if width > max || height > max {
                return Err(format!("capture dimensions exceed GPU limit {max}"));
            }
            let ctx = egui::Context::default();
            let gpu =
                egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, None, 1, false);
            Ok(Self {
                ctx,
                gpu,
                device,
                queue,
                width,
                height,
            })
        })
    }
    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    /// Runs two UI passes for initial font/layout updates. The callback must not advance game time.
    pub fn save(
        &mut self,
        path: &Path,
        mut draw: impl FnMut(&egui::Context),
    ) -> Result<(), String> {
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Scene capture target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        for _ in 0..2 {
            let output = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(self.width as f32, self.height as f32),
                    )),
                    time: Some(0.0),
                    ..Default::default()
                },
                |ctx| draw(ctx),
            );
            let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
            for (id, delta) in &output.textures_delta.set {
                self.gpu
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: [self.width, self.height],
                pixels_per_point: output.pixels_per_point,
            };
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let commands =
                self.gpu
                    .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
            let view = target.create_view(&Default::default());
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Scene capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                self.gpu.render(&mut pass.forget_lifetime(), &jobs, &screen);
            }
            self.queue
                .submit(commands.into_iter().chain(Some(encoder.finish())));
            for id in &output.textures_delta.free {
                self.gpu.free_texture(id);
            }
        }
        let stride = (self.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Capture readback"),
            size: (stride * self.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(self.height),
                },
            },
            size,
        );
        self.queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = buffer.slice(..).get_mapped_range();
        let rgba: Vec<u8> = mapped
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..(self.width * 4) as usize].iter().copied())
            .collect();
        image::save_buffer(
            path,
            &rgba,
            self.width,
            self.height,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())
    }
}
