//! Image ownership and registration. No loading, prediction or eviction policy.
use engine_image_resources::{DecodedImage, TextureCommand};
use std::collections::BTreeMap;
#[cfg(feature = "gpu")]
pub(crate) mod gpu;

pub(crate) enum StoredImage {
    Managed(egui::TextureHandle),
    #[cfg(feature = "gpu")]
    Gpu(gpu::ImageLease),
}
impl StoredImage {
    pub(crate) fn id(&self) -> egui::TextureId {
        match self {
            Self::Managed(image) => image.id(),
            #[cfg(feature = "gpu")]
            Self::Gpu(_) => unreachable!("direct sprites do not use egui texture IDs"),
        }
    }
    fn size_vec2(&self) -> egui::Vec2 {
        match self {
            Self::Managed(image) => image.size_vec2(),
            #[cfg(feature = "gpu")]
            Self::Gpu(image) => egui::vec2(image.size[0] as f32, image.size[1] as f32),
        }
    }
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu(&self) -> Option<u64> {
        match self {
            Self::Gpu(image) => Some(image.id),
            Self::Managed(_) => None,
        }
    }
}

#[derive(Default)]
pub struct TextureStore {
    textures: BTreeMap<String, StoredImage>,
    #[cfg(feature = "gpu")]
    gpu: Option<gpu::Uploader>,
}
impl TextureStore {
    /// Select the concrete direct-sprite backend at the platform boundary.
    /// It uses the existing renderer's device/queue; no second GPU or executor.
    #[cfg(feature = "gpu")]
    pub fn with_gpu(state: egui_wgpu::RenderState) -> Self {
        Self {
            textures: BTreeMap::new(),
            gpu: Some(gpu::Uploader::new(state)),
        }
    }

    pub fn validate_image(&self, image: &DecodedImage) -> Result<(), String> {
        image.validate()?;
        #[cfg(feature = "gpu")]
        if let Some(gpu) = &self.gpu {
            gpu.validate_size(image.size)?;
        }
        Ok(())
    }
    /// Registers predicted or visible images without drawing or waiting for the GPU.
    pub fn apply(&mut self, ctx: &egui::Context, command: TextureCommand) -> Result<(), String> {
        match command {
            TextureCommand::Register { path, image } => {
                self.validate_image(&image)?;
                #[cfg(feature = "gpu")]
                if let Some(gpu) = &self.gpu {
                    self.textures
                        .insert(path, StoredImage::Gpu(gpu.upload(image)));
                    ctx.request_repaint();
                    return Ok(());
                }
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [image.size[0] as usize, image.size[1] as usize],
                    &image.rgba,
                );
                self.insert(ctx, path, color);
            }
            TextureCommand::Release { path } => {
                self.textures.remove(&path);
            }
        }
        Ok(())
    }
    pub fn insert(&mut self, ctx: &egui::Context, path: String, image: egui::ColorImage) {
        self.textures.insert(
            path.clone(),
            StoredImage::Managed(ctx.load_texture(path, image, egui::TextureOptions::LINEAR)),
        );
        ctx.request_repaint();
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.textures.retain(|path, _| keep(path));
    }
    pub fn images(&self) -> BTreeMap<String, egui::Vec2> {
        self.textures
            .iter()
            .map(|(path, image)| (path.clone(), image.size_vec2()))
            .collect()
    }
    pub(crate) fn get(&self, path: &str) -> Option<&StoredImage> {
        self.textures.get(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_image_resources::DecodedImage;
    #[test]
    fn registers_without_drawing_and_releases_through_messages() {
        let ctx = egui::Context::default();
        let mut store = TextureStore::default();
        let output = ctx.run(Default::default(), |ctx| {
            store
                .apply(
                    ctx,
                    TextureCommand::Register {
                        path: "future".into(),
                        image: DecodedImage {
                            size: [1, 1],
                            rgba: vec![255; 4],
                        },
                    },
                )
                .unwrap();
        });
        let texture = store.get("future").unwrap().id();
        assert!(output
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| *id == texture));
        let output = ctx.run(Default::default(), |ctx| {
            store
                .apply(
                    ctx,
                    TextureCommand::Release {
                        path: "future".into(),
                    },
                )
                .unwrap();
        });
        assert!(output.textures_delta.free.contains(&texture));
        assert!(store.images().is_empty());
    }
}
