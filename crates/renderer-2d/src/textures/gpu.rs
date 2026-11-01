//! GPU image allocation/upload/release. No scene decisions or CPU color conversion.
use crate::sprites::{self, Resources};
use engine_image_resources::DecodedImage;
use std::collections::HashMap;

pub(crate) struct Image {
    pub _texture: wgpu::Texture,
    pub binding: wgpu::BindGroup,
}

pub(crate) struct ImageResources {
    layout: wgpu::BindGroupLayout,
    pub images: HashMap<u64, Image>,
}

pub(super) struct Uploader {
    state: egui_wgpu::RenderState,
}
impl Uploader {
    pub fn new(state: egui_wgpu::RenderState) -> Self {
        {
            let mut renderer = state.renderer.write();
            if renderer
                .callback_resources
                .get::<ImageResources>()
                .is_none()
            {
                let device = &state.device;
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Sprite image layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    }],
                });

                let sprites = Resources::new(device, state.target_format, &layout);
                renderer.callback_resources.insert(ImageResources {
                    layout,
                    images: HashMap::new(),
                });
                renderer.callback_resources.insert(sprites);
            }
        }
        Self { state }
    }
    pub fn validate_size(&self, size: [u32; 2]) -> Result<(), String> {
        let max = self.state.device.limits().max_texture_dimension_2d;
        if size.iter().any(|side| *side > max) {
            return Err(format!(
                "Image {}x{} exceeds GPU texture limit {max}",
                size[0], size[1]
            ));
        }
        Ok(())
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.state.target_format
    }
    pub fn upload(&self, image: DecodedImage) -> ImageLease {
        let size = wgpu::Extent3d {
            width: image.size[0],
            height: image.size[1],
            depth_or_array_layers: 1,
        };
        let texture = self.state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Straight RGBA sprite"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.state.queue.write_texture(
            texture.as_image_copy(),
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        let mut renderer = self.state.renderer.write();
        let resources = renderer
            .callback_resources
            .get_mut::<ImageResources>()
            .expect("sprite backend installed");
        let binding = self
            .state
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Sprite image"),
                layout: &resources.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
            });
        let id = sprites::next_id();
        resources.images.insert(
            id,
            Image {
                _texture: texture,
                binding,
            },
        );
        // Flush the transfer even for predicted images that have not been drawn.
        // Later UI/sprite rendering is submitted to this SAME queue. No CPU wait.
        self.state.queue.submit([]);
        ImageLease {
            id,
            size: image.size,
            state: self.state.clone(),
        }
    }
}

/// TextureStore owns this lease. egui's backend slot is only storage needed by
/// its callback API, not an image cache or an independent lifetime owner.
pub(crate) struct ImageLease {
    pub id: u64,
    pub size: [u32; 2],
    state: egui_wgpu::RenderState,
}
impl Drop for ImageLease {
    fn drop(&mut self) {
        if let Some(resources) = self
            .state
            .renderer
            .write()
            .callback_resources
            .get_mut::<ImageResources>()
        {
            resources.images.remove(&self.id);
        }
    }
}
