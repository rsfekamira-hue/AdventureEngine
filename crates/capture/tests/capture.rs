use engine_capture::Capture;
#[test]
fn zero_dimensions_fail_without_gpu() {
    assert!(Capture::new(0, 10).is_err());
    assert!(Capture::new(10, 0).is_err());
}
#[test]
#[ignore = "Requires GPU; cargo test -p engine-capture -- --ignored"]
fn png_readback_handles_padded_rows_and_repeated_frames() {
    let mut capture = Capture::new(257, 33).unwrap();
    let dir = std::env::temp_dir().join(format!("engine-capture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("frame.png");
    for color in [egui::Color32::RED, egui::Color32::GREEN] {
        capture
            .save(&path, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(color))
                    .show(ctx, |_| {});
            })
            .unwrap();
        let image = image::open(&path).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (257, 33));
        assert_eq!(image.get_pixel(0, 0).0, color.to_array());
        assert_eq!(image.get_pixel(256, 32).0, color.to_array());
    }
    assert!(capture
        .save(&dir.join("missing/frame.png"), |_| {})
        .is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
