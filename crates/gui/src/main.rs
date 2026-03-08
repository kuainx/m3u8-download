mod app;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([700.0, 500.0])
            .with_title("M3U8 Downloader"),
        ..Default::default()
    };

    eframe::run_native(
        "M3U8 Downloader",
        native_options,
        Box::new(|cc| Ok(Box::new(app::M3u8App::new(cc)))),
    )
}
