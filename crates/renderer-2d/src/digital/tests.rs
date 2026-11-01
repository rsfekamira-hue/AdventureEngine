use super::*;
use crate::{textures::TextureStore, DrawCommand, DrawList, Renderer2D};

#[test]
fn gpu_panels_keep_independent_colors_clip_animate_and_reuse_glyphs() {
    let mut capture = engine_capture::Capture::new(420, 240).unwrap();
    let ctx = capture.context().clone();
    let font = egui::FontDefinitions::default()
        .font_data
        .into_values()
        .next()
        .unwrap()
        .font
        .to_vec();
    let renderer = Renderer2D::new(&ctx, font);
    let textures = TextureStore::with_gpu(capture.render_state());
    let directory = std::path::Path::new("target/test-artifacts/digital");
    std::fs::create_dir_all(directory).unwrap();
    for (index, seconds) in [0., 4., 4., 4.].into_iter().enumerate() {
        capture
            .save(&directory.join(format!("{index}.png")), |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(30, 40, 60)))
                    .show(ctx, |ui| {
                        let mut p =
                            DrawList::new(Rect::from_min_max(pos2(0., 0.), pos2(420., 240.)));
                        let alpha = if index == 3 { 0. } else { 1. };
                        for (x, color) in
                            [(10., [0.1, 0.9, 1., alpha]), (220., [1., 0.2, 0.3, alpha])]
                        {
                            let rect = Rect::from_min_size(pos2(x, 10.), vec2(190., 210.));
                            p.with_clip_rect(rect, |p| {
                                p.push(DrawCommand::Digital(DigitalDraw {
                                    surface: DigitalSurface::Panel,
                                    seconds,
                                    rect,
                                    color,
                                    phase: 1.,
                                    progress: Some(0.5),
                                    text: vec![DigitalText {
                                        position: [0.08, 0.2],
                                        size: 0.12,
                                        text: "DATA 50%".into(),
                                    }],
                                }))
                            });
                        }
                        renderer.render(ui, p.finish(), &textures);
                    });
            })
            .unwrap();
        let state = capture.render_state();
        let gpu = state.renderer.read();
        let resources = gpu.callback_resources.get::<Resources>().unwrap();
        assert_eq!(resources.meshes.len(), 2);
        assert_eq!(resources.glyphs.len(), 7);
    }
    let read = |n| {
        image::open(directory.join(format!("{n}.png")))
            .unwrap()
            .to_rgba8()
    };
    let still = read(0);
    let moving = read(1);
    let repeated = read(2);
    assert_ne!(
        still, moving,
        "decorative GPU motion must change pixels at fixed phase"
    );
    assert_eq!(moving, repeated, "same clock must replay deterministically");
    let left = moving.get_pixel(50, 165);
    let right = moving.get_pixel(260, 165);
    assert!(
        left[1] > left[0] && right[0] > right[1],
        "panels must not share overwritten vertex data"
    );
    assert_eq!(
        moving.get_pixel(205, 120),
        moving.get_pixel(415, 120),
        "no paint outside panel clip"
    );
    let hidden = read(3);
    assert_eq!(
        hidden.get_pixel(50, 165),
        hidden.get_pixel(205, 120),
        "zero alpha must hide panel, text and progress together"
    );
}
