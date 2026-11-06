pub mod app;
pub mod config;
pub mod db;
pub mod models;
pub mod ui;

use app::PgMiniApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("pg-mini - Lightweight PostgreSQL Manager")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([700.0, 480.0])
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "pg-mini",
        native_options,
        Box::new(|cc| Ok(Box::new(PgMiniApp::new(cc)))),
    )
}
