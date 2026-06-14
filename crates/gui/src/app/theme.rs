use eframe::egui;

pub(super) fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let font_path = std::path::Path::new("C:\\Windows\\Fonts\\msyh.ttc");
    if font_path.exists() {
        if let Ok(font_data) = std::fs::read(font_path) {
            fonts.font_data.insert(
                "msyh".to_owned(),
                egui::FontData::from_owned(font_data).into(),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "msyh".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("msyh".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

pub(super) fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::from_rgb(22, 22, 30);
    visuals.panel_fill = egui::Color32::from_rgb(22, 22, 30);
    visuals.faint_bg_color = egui::Color32::from_rgb(32, 32, 42);
    visuals.extreme_bg_color = egui::Color32::from_rgb(16, 16, 22);

    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);

    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 48);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 42, 58);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(50, 50, 68);

    visuals.selection.bg_fill = egui::Color32::from_rgb(50, 100, 200);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 160, 255));

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);

    ctx.set_style(style);
}
