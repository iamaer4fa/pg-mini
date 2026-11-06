use egui::{Color32, Rounding, Stroke};

pub fn setup_custom_theme(ctx: &egui::Context, dark_mode: bool) {
    if dark_mode {
        let mut visuals = egui::Visuals::dark();
        let btn_rounding = Rounding::same(5.0);
        visuals.widgets.inactive.rounding = btn_rounding;
        visuals.widgets.hovered.rounding = btn_rounding;
        visuals.widgets.active.rounding = btn_rounding;
        visuals.widgets.open.rounding = btn_rounding;
        visuals.window_rounding = Rounding::same(8.0);
        visuals.menu_rounding = Rounding::same(6.0);

        visuals.panel_fill = Color32::from_rgb(22, 25, 32);
        visuals.window_fill = Color32::from_rgb(25, 29, 38);
        visuals.faint_bg_color = Color32::from_rgb(28, 32, 42);
        visuals.extreme_bg_color = Color32::from_rgb(16, 18, 24);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 38, 50);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(50, 56, 72));
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(220, 226, 238));

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 52, 68);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(75, 85, 110));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);

        visuals.widgets.active.bg_fill = Color32::from_rgb(56, 64, 84);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(90, 105, 135));

        visuals.selection.bg_fill = Color32::from_rgb(37, 99, 235);
        ctx.set_visuals(visuals);
    } else {
        let mut visuals = egui::Visuals::light();
        let btn_rounding = Rounding::same(5.0);
        visuals.widgets.inactive.rounding = btn_rounding;
        visuals.widgets.hovered.rounding = btn_rounding;
        visuals.widgets.active.rounding = btn_rounding;
        visuals.widgets.open.rounding = btn_rounding;
        visuals.window_rounding = Rounding::same(8.0);
        visuals.menu_rounding = Rounding::same(6.0);
        ctx.set_visuals(visuals);
    }
}
