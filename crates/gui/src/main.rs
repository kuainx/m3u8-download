#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use egui::IconData;
use image::GenericImageView;

fn main() -> eframe::Result<()> {
    let icon_bytes = include_bytes!("../../../logo.ico");
    let icon_image = image::load_from_memory(icon_bytes).expect("Failed to load icon");
    let (width, height) = icon_image.dimensions();
    let icon_rgba8 = icon_image.to_rgba8().into_raw();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([700.0, 500.0])
            .with_title("M3U8 Downloader")
            .with_icon(IconData {
                rgba: icon_rgba8,
                width,
                height,
            }),
        ..Default::default()
    };

    eframe::run_native(
        "M3U8 Downloader",
        native_options,
        Box::new(|cc| Ok(Box::new(app::M3u8App::new(cc)))),
    )
}
