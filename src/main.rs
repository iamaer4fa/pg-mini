pub mod app;
pub mod config;
pub mod db;
pub mod models;
pub mod ui;

use app::PgMiniApp;

fn main() -> eframe::Result<()> {
    let icon_image = image::load_from_memory(include_bytes!("../assets/pg-mini.png"))
        .expect("embedded pg-mini icon must be a valid PNG")
        .into_rgba8();
    let (icon_width, icon_height) = icon_image.dimensions();
    let icon = egui::IconData {
        rgba: icon_image.into_raw(),
        width: icon_width,
        height: icon_height,
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pg-mini - Lightweight PostgreSQL Manager")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([700.0, 480.0])
            .with_icon(icon)
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "pg-mini",
        native_options,
        Box::new(|cc| Ok(Box::new(PgMiniApp::new(cc)))),
    )
}
