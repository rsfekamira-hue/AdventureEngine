use super::*;
use crate::textures::gpu::ImageResources;
use crate::{textures::TextureStore, DrawList, Renderer2D};
use engine_image_resources::{DecodedImage, TextureCommand};

fn fixture() -> DecodedImage {
    let mut rgba = Vec::new();
    for y in 0..33 {
        for x in 0..257 {
            rgba.extend_from_slice(&[
                x as u8,
                (255 - x.min(255)) as u8,
                (y * 7) as u8,
                [0, 1, 32, 64, 128, 192, 254, 255][y % 8],
            ]);
        }
    }
    DecodedImage {
        size: [257, 33],
        rgba,
    }
}

#[test]
fn direct_sprites_match_reference_alpha_crop_rotation_and_layer_order() {
    let mut capture = engine_capture::Capture::new(511, 257).unwrap();
    let ctx = capture.context().clone();
    let renderer = Renderer2D {
        context: ctx.clone(),
        particles: Default::default(),
    };
    let directory = std::path::Path::new("target/test-artifacts/direct-sprites");
    std::fs::create_dir_all(directory).unwrap();
    for direct in [false, true] {
        let mut images = if direct {
            TextureStore::with_gpu(capture.render_state())
        } else {
            TextureStore::default()
        };
        images
            .apply(
                &ctx,
                TextureCommand::Register {
                    path: "image".into(),
                    image: fixture(),
                },
            )
            .unwrap();
        let path = directory.join(if direct {
            "direct.png"
        } else {
            "reference.png"
        });
        capture
            .save(&path, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(Color32::from_rgb(41, 53, 71)))
                    .show(ctx, |ui| {
                        let mut list = DrawList::new(ui.max_rect());
                        list.sprite(
                            "image",
                            Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(257.0, 100.0)),
                            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            0.0,
                            Color32::WHITE,
                        );
                        list.with_clip_rect(
                            Rect::from_min_max(egui::pos2(30.0, 112.0), egui::pos2(470.0, 245.0)),
                            |list| {
                                list.sprite(
                                    "image",
                                    Rect::from_min_size(
                                        egui::pos2(-20.0, 110.0),
                                        egui::vec2(470.0, 100.0),
                                    ),
                                    Rect::from_min_max(
                                        egui::pos2(0.18, 0.12),
                                        egui::pos2(0.87, 0.93),
                                    ),
                                    0.16,
                                    Color32::from_rgba_unmultiplied(210, 180, 240, 150),
                                );
                            },
                        );
                        // UI drawn after a sprite must cover it in both backends.
                        list.rect(
                            Rect::from_min_size(egui::pos2(70.0, 70.0), egui::vec2(65.0, 25.0)),
                            0.0,
                            Color32::from_rgb(180, 40, 60),
                            egui::Stroke::NONE,
                            egui::StrokeKind::Inside,
                        );
                        renderer.render(ui, list.finish(), &images);
                    });
            })
            .unwrap();
        if direct {
            // Neither the upload nor rendering registered an egui-managed image.
            assert!(images.get("image").unwrap().gpu().is_some());
        }
    }
    let reference = image::open(directory.join("reference.png"))
        .unwrap()
        .into_rgba8();
    let direct = image::open(directory.join("direct.png"))
        .unwrap()
        .into_rgba8();
    let differences: Vec<_> = reference
        .as_raw()
        .iter()
        .zip(direct.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    let max = differences.iter().copied().max().unwrap();
    let mean = differences.iter().map(|v| *v as f64).sum::<f64>() / differences.len() as f64;
    eprintln!("direct sprite/reference pixel difference max={max} mean={mean:.4}");
    assert!(
        max <= 4,
        "alpha/filtering/color mismatch: max={max}, mean={mean}"
    );
    assert_eq!(direct.get_pixel(90, 80).0, [180, 40, 60, 255]);
    assert_eq!(direct.get_pixel(10, 220), reference.get_pixel(10, 220));
}

#[test]
fn high_resolution_upload_stays_raw_and_replacement_release_are_owned_by_store() {
    let capture = engine_capture::Capture::new(64, 64).unwrap();
    let ctx = capture.context();
    let state = capture.render_state();
    let mut images = TextureStore::with_gpu(state.clone());
    for side in [2048u32, 4096] {
        let image = DecodedImage {
            size: [side, side],
            rgba: vec![128; side as usize * side as usize * 4],
        };
        let started = std::time::Instant::now();
        images
            .apply(
                ctx,
                TextureCommand::Register {
                    path: "large".into(),
                    image,
                },
            )
            .unwrap();
        eprintln!(
            "{side}x{side} direct upload CPU submit: {:.2} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    let first = images.get("large").unwrap().gpu().unwrap();
    {
        let renderer = state.renderer.read();
        let resources = renderer.callback_resources.get::<ImageResources>().unwrap();
        let texture = &resources.images[&first]._texture;
        assert_eq!(texture.format(), wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(
            texture.usage(),
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST
        );
        assert_eq!(resources.images.len(), 1);
    }
    let excessive = state.device.limits().max_texture_dimension_2d + 1;
    assert!(images
        .apply(
            ctx,
            TextureCommand::Register {
                path: "invalid".into(),
                image: DecodedImage {
                    size: [excessive, 1],
                    rgba: vec![0; excessive as usize * 4]
                },
            }
        )
        .is_err());
    images
        .apply(
            ctx,
            TextureCommand::Register {
                path: "large".into(),
                image: fixture(),
            },
        )
        .unwrap();
    let second = images.get("large").unwrap().gpu().unwrap();
    assert_ne!(first, second);
    assert_eq!(
        state
            .renderer
            .read()
            .callback_resources
            .get::<ImageResources>()
            .unwrap()
            .images
            .len(),
        1
    );
    assert!(!state
        .renderer
        .read()
        .callback_resources
        .get::<ImageResources>()
        .unwrap()
        .images
        .contains_key(&first));
    images
        .apply(
            ctx,
            TextureCommand::Release {
                path: "large".into(),
            },
        )
        .unwrap();
    assert!(state
        .renderer
        .read()
        .callback_resources
        .get::<ImageResources>()
        .unwrap()
        .images
        .is_empty());
    images
        .apply(
            ctx,
            TextureCommand::Register {
                path: "drop".into(),
                image: fixture(),
            },
        )
        .unwrap();
    drop(images);
    assert!(state
        .renderer
        .read()
        .callback_resources
        .get::<ImageResources>()
        .unwrap()
        .images
        .is_empty());
    state.device.poll(wgpu::Maintain::Wait);
}
