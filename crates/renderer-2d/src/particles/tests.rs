use super::*;
use crate::{textures::TextureStore, DrawCommand, DrawList, Renderer2D};

fn emitter() -> ParticleEmitter {
    ParticleEmitter {
        seed: 71,
        count: 20,
        lifetime_ms: 1500.,
        shape: ParticleShape::Quad,
        spawn: [0.1, 0.1, 0.8, 0.8],
        velocity: [0.2, -0.1],
        spread: [0.1, 0.1],
        acceleration: [0., 0.03],
        destination: Some([0.2, 0.2, 0.6, 0.6]),
        size: [0.06, 0.03],
        color: [0.1, 1., 0.5, 0.8],
        glow: 0.5,
        glitch: 0.15,
        trails: 2,
        trail_ms: 50.,
    }
}
fn renderer(ctx: &egui::Context) -> Renderer2D {
    let font = egui::FontDefinitions::default()
        .font_data
        .into_values()
        .next()
        .unwrap()
        .font
        .to_vec();
    Renderer2D::new(ctx, font)
}
fn read_instances(
    state: &egui_wgpu::RenderState,
    owner: &Arc<Vec<ParticleEmitter>>,
) -> Vec<[f32; 12]> {
    let gpu = state.renderer.read();
    let resources = gpu.callback_resources.get::<Resources>().unwrap();
    let source = &resources.groups[&(Arc::as_ptr(owner) as usize, 0)].particles;
    let staging = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: source.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = state.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, source.size());
    state.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    state.device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let values = bytemuck::cast_slice(&staging.slice(..).get_mapped_range()).to_vec();
    staging.unmap();
    values
}

#[test]
fn gpu_simulation_matches_reference_replays_and_reuses_buffers() {
    let mut capture = engine_capture::Capture::new(320, 240).unwrap();
    let state = capture.render_state();
    let ctx = capture.context().clone();
    let renderer = renderer(&ctx);
    let textures = TextureStore::with_gpu(state.clone());
    let emitters = Arc::new(vec![emitter()]);
    std::fs::create_dir_all("target/test-artifacts/particles").unwrap();
    let mut first = Vec::new();
    for time in [850., 950., 850.] {
        capture
            .save(
                std::path::Path::new("target/test-artifacts/particles/replay.png"),
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut draw = DrawList::new(ui.max_rect());
                        draw.push(DrawCommand::Particles(ParticleDraw {
                            emitters: emitters.clone(),
                            rect: ui.max_rect(),
                            milliseconds: time,
                            fade: 1.,
                            tint: [1.; 4],
                        }));
                        renderer.render(ui, draw.finish(), &textures);
                    });
                },
            )
            .unwrap();
        let actual = read_instances(&state, &emitters);
        let mut expected = Vec::new();
        emitters[0].sample(time as f64, 1., &mut expected);
        let actual_visible: Vec<_> = actual.iter().filter(|v| v[7] > 0.).collect();
        assert_eq!(actual_visible.len(), expected.len());
        for (gpu, cpu) in actual_visible.iter().zip(&expected) {
            for (a, b) in [
                (gpu[0], cpu.position[0]),
                (gpu[1], cpu.position[1]),
                (gpu[2], cpu.size),
                (gpu[7], cpu.color[3]),
            ] {
                assert!((a - b).abs() < 0.0002, "GPU {a}, reference {b}");
            }
        }
        assert_eq!(
            state
                .renderer
                .read()
                .callback_resources
                .get::<Resources>()
                .unwrap()
                .groups
                .len(),
            1
        );
        if first.is_empty() {
            first = actual;
        } else if time == 850. {
            assert_eq!(first, actual);
        } else {
            assert_ne!(first, actual);
        }
    }
}

#[test]
fn particles_overflow_emitter_bounds_but_keep_screen_clip_and_ui_order() {
    let mut capture = engine_capture::Capture::new(256, 160).unwrap();
    let state = capture.render_state();
    let ctx = capture.context().clone();
    let renderer = renderer(&ctx);
    let textures = TextureStore::with_gpu(state);
    let mut e = emitter();
    e.spawn = [1.2, 0.5, 0., 0.];
    e.velocity = [0., 0.];
    e.spread = [0., 0.];
    e.acceleration = [0., 0.];
    e.destination = None;
    e.size = [0.2, 0.2];
    e.trails = 0;
    let mut outside = e.clone();
    outside.spawn[0] = 2.2;
    let emitters = Arc::new(vec![e, outside]);
    let path = std::path::Path::new("target/test-artifacts/particles/overflow.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    capture
        .save(path, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut draw = DrawList::new(Rect::from_min_max(
                    egui::pos2(20., 20.),
                    egui::pos2(220., 140.),
                ));
                draw.push(DrawCommand::Particles(ParticleDraw {
                    emitters: emitters.clone(),
                    rect: Rect::from_min_size(egui::pos2(50., 30.), egui::vec2(80., 80.)),
                    milliseconds: 850.,
                    fade: 1.,
                    tint: [1.; 4],
                }));
                draw.rect_filled(
                    Rect::from_min_max(egui::pos2(135., 70.), egui::pos2(160., 90.)),
                    0.,
                    egui::Color32::RED,
                );
                renderer.render(ui, draw.finish(), &textures);
            });
        })
        .unwrap();
    let image = image::open(path).unwrap().to_rgba8();
    assert!(
        image
            .enumerate_pixels()
            .any(|(x, y, p)| x > 130 && y < 70 && i16::from(p[1]) > i16::from(p[0]) + 20),
        "particles must extend beyond the emitter rectangle"
    );
    assert_eq!(
        image.get_pixel(145, 80).0,
        [255, 0, 0, 255],
        "UI remains in front"
    );
    assert!(
        image.get_pixel(226, 65).0[1] < 80,
        "screen clip must still hide particles outside the stage"
    );
}

#[test]
fn text_atlas_is_built_once_for_shared_definitions() {
    let font = egui::FontDefinitions::default()
        .font_data
        .into_values()
        .next()
        .unwrap()
        .font
        .to_vec();
    let font = FontArc::try_from_vec(font).unwrap();
    let a = atlas(
        Some(&font),
        &ParticleShape::Phrases {
            phrases: vec!["NAME".into(), "NEW NAME".into()],
            change_ms: 100.,
        },
    );
    assert_eq!(a.glyphs.len(), 2);
    assert!(a.pixels.iter().any(|v| *v > 0));
    assert!(a.glyphs[1].metrics[0] > a.glyphs[0].metrics[0]);
    let mut painter = ParticlePainter::new(font);
    let mut e = emitter();
    e.shape = ParticleShape::Text {
        characters: "01AB".into(),
        change_ms: 100.,
    };
    let emitters = Arc::new(vec![e]);
    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 240.));
    for time in [0., 500., 1000.] {
        painter.callbacks(
            rect,
            wgpu::TextureFormat::Rgba8Unorm,
            ParticleDraw {
                emitters: emitters.clone(),
                rect,
                milliseconds: time,
                fade: 1.,
                tint: [1.; 4],
            },
        );
        assert_eq!(painter.atlases.len(), 1);
    }
    drop(emitters);
    let defs = Arc::new(vec![emitter()]);
    painter.callbacks(
        rect,
        wgpu::TextureFormat::Rgba8Unorm,
        ParticleDraw {
            emitters: defs,
            rect,
            milliseconds: 0.,
            fade: 1.,
            tint: [1.; 4],
        },
    );
    assert_eq!(
        painter.atlases.len(),
        1,
        "unused definition atlases must be reclaimed"
    );
}
