//! egui texture ownership and registration. No loading, prediction or eviction policy.
use engine_image_resources::TextureCommand;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct TextureStore {
    textures: BTreeMap<String, egui::TextureHandle>,
}
impl TextureStore {
    /// eframe uploads all texture deltas before drawing, including not-yet-visible images.
    pub fn apply(&mut self, ctx: &egui::Context, command: TextureCommand) {
        match command {
            TextureCommand::Register { path, image } => {
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
    }
    pub fn insert(&mut self, ctx: &egui::Context, path: String, image: egui::ColorImage) {
        self.textures.insert(
            path.clone(),
            ctx.load_texture(path, image, egui::TextureOptions::LINEAR),
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
    pub(crate) fn get(&self, path: &str) -> Option<&egui::TextureHandle> {
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
            store.apply(
                ctx,
                TextureCommand::Register {
                    path: "future".into(),
                    image: DecodedImage {
                        size: [1, 1],
                        rgba: vec![255; 4],
                    },
                },
            );
        });
        let texture = store.get("future").unwrap().id();
        assert!(output
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| *id == texture));
        let output = ctx.run(Default::default(), |ctx| {
            store.apply(
                ctx,
                TextureCommand::Release {
                    path: "future".into(),
                },
            );
        });
        assert!(output.textures_delta.free.contains(&texture));
        assert!(store.images().is_empty());
    }
}
