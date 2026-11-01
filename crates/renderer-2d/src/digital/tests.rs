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

#[test]
fn confirmation_press_recesses_flashes_once_and_stays_sealed() {
    let mut capture = engine_capture::Capture::new(400, 200).unwrap();
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
    let directory = std::path::Path::new("target/test-artifacts/confirmation");
    std::fs::create_dir_all(directory).unwrap();
    for (index, (phase, seconds)) in [(0., 0.), (0.38, 1.37), (0.56, 2.02), (1., 3.6), (1., 12.)]
        .into_iter()
        .enumerate()
    {
        capture
            .save(&directory.join(format!("{index}.png")), |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(20, 20, 30)))
                    .show(ctx, |ui| {
                        let mut p =
                            DrawList::new(Rect::from_min_max(pos2(0., 0.), pos2(400., 200.)));
                        p.push(DrawCommand::Digital(DigitalDraw {
                            surface: DigitalSurface::Press,
                            rect: Rect::from_min_size(pos2(20., 40.), vec2(360., 100.)),
                            seconds,
                            color: [0.86, 0.1, 0.3, 1.],
                            phase,
                            progress: None,
                            text: vec![DigitalText {
                                position: [0.12, 0.3],
                                size: 0.25,
                                text: "CONFIRM".into(),
                            }],
                        }));
                        renderer.render(ui, p.finish(), &textures);
                    });
            })
            .unwrap();
    }
    let read = |i| {
        image::open(directory.join(format!("{i}.png")))
            .unwrap()
            .to_rgba8()
    };
    let idle = read(0);
    let pressed = read(1);
    let pulse = read(2);
    let sealed = read(3);
    let background = idle.get_pixel(5, 5);
    assert_ne!(idle.get_pixel(200, 45), background);
    assert_eq!(
        pressed.get_pixel(200, 45),
        background,
        "agreement plate must visibly recess"
    );
    let red = |image: &image::RgbaImage| image.pixels().map(|p| u64::from(p[0])).sum::<u64>();
    assert!(
        red(&pulse) > red(&idle),
        "acceptance pulse must brighten the panel"
    );
    assert_ne!(
        idle, sealed,
        "acceptance must leave a persistent distinct state"
    );
    assert_eq!(
        sealed,
        read(4),
        "one-shot press and flash must not loop after confirmation"
    );
}
