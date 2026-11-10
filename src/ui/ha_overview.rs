use crate::ha::{HaNode, NodeRole};
use egui::{Color32, RichText, Ui};

pub fn render_ha_overview(ui: &mut Ui, nodes: &[HaNode], is_busy: bool) -> bool {
    let mut refresh = false;
    ui.horizontal(|ui| {
        ui.heading("High Availability Overview");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!is_busy, egui::Button::new("↻ Refresh roles"))
                .clicked()
            {
                refresh = true;
            }
        });
    });
    ui.label(
        RichText::new(
            "PostgreSQL role detection for connected servers. Cluster membership and replication health are not checked yet.",
        )
        .small()
        .color(Color32::GRAY),
    );
    ui.separator();

    if nodes.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("Connect to a PostgreSQL server to detect its HA role.");
        });
        return refresh;
    }

    egui::Grid::new("ha_overview_nodes")
        .striped(true)
        .min_col_width(100.0)
        .show(ui, |ui| {
            for label in ["Server", "Endpoint / database", "Role", "Status"] {
                ui.label(RichText::new(label).strong());
            }
            ui.end_row();

            for node in nodes {
                ui.horizontal(|ui| {
                    if node.is_active {
                        ui.label(RichText::new("●").color(Color32::LIGHT_BLUE));
                    }
                    ui.label(&node.name);
                });
                ui.label(format!("{}/{}", node.host_port, node.database));

                match &node.status {
                    Some(status) => {
                        let role_color = match status.role {
                            NodeRole::Primary => Color32::from_rgb(100, 220, 130),
                            NodeRole::Standby => Color32::from_rgb(100, 180, 255),
                        };
                        ui.label(
                            RichText::new(status.role.label())
                                .color(role_color)
                                .strong(),
                        );
                        ui.label("Connected");
                    }
                    None => {
                        ui.label(RichText::new("Unknown").color(Color32::YELLOW));
                        ui.label(if let Some(error) = &node.error {
                            error.as_str()
                        } else if node.is_refreshing {
                            "Detecting role..."
                        } else {
                            "Role not detected"
                        });
                    }
                }
                ui.end_row();
            }
        });

    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "A primary/standby label describes each server independently. pg-mini does not yet verify that listed servers belong to the same cluster.",
        )
        .small()
        .color(Color32::GRAY),
    );
    refresh
}
