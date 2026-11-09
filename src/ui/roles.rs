use crate::models::{RoleForm, RoleItem};
use egui::{Color32, RichText, Ui, Window};

#[derive(Clone, Copy, PartialEq)]
enum RoleFilter {
    All,
    Login,
    Group,
}

#[derive(Default)]
pub struct RolesViewState {
    filter: Option<RoleFilter>,
    form: Option<RoleForm>,
}

pub enum RolesAction {
    Refresh,
    Save(RoleForm),
    Drop(String),
}

impl RolesViewState {
    pub fn render(
        &mut self,
        ui: &mut Ui,
        roles: &[RoleItem],
        is_busy: bool,
    ) -> Option<RolesAction> {
        let mut action = None;
        let filter = self.filter.get_or_insert(RoleFilter::All);

        ui.horizontal(|ui| {
            ui.heading("Users & Roles");
            ui.label(
                RichText::new("Login roles act as users; NOLOGIN roles are group roles.")
                    .size(11.0)
                    .color(Color32::GRAY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!is_busy, egui::Button::new("↻ Refresh"))
                    .clicked()
                {
                    action = Some(RolesAction::Refresh);
                }
                if ui
                    .add_enabled(!is_busy, egui::Button::new("➕ Create Role"))
                    .clicked()
                {
                    self.form = Some(RoleForm::new());
                }
            });
        });
        ui.separator();

        ui.horizontal(|ui| {
            for (label, selected) in [
                ("All", RoleFilter::All),
                ("Login roles (users)", RoleFilter::Login),
                ("Group roles", RoleFilter::Group),
            ] {
                if ui.selectable_label(*filter == selected, label).clicked() {
                    *filter = selected;
                }
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("postgres_role_grid")
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for header in [
                        "Name",
                        "Type",
                        "Member of",
                        "Attributes",
                        "Connection limit",
                        "Valid until",
                        "Actions",
                    ] {
                        ui.label(RichText::new(header).strong());
                    }
                    ui.end_row();

                    for role in roles.iter().filter(|role| match filter {
                        RoleFilter::All => true,
                        RoleFilter::Login => role.can_login,
                        RoleFilter::Group => !role.can_login,
                    }) {
                        ui.label(RichText::new(&role.name).strong());
                        ui.label(if role.can_login { "Login" } else { "Group" });
                        ui.label(if role.memberships.is_empty() {
                            "—".to_string()
                        } else {
                            role.memberships.join(", ")
                        });

                        let attributes = [
                            (role.is_superuser, "Superuser"),
                            (role.can_create_db, "Create DB"),
                            (role.can_create_role, "Create role"),
                            (role.can_replicate, "Replication"),
                            (role.can_bypass_rls, "Bypass RLS"),
                        ]
                        .into_iter()
                        .filter_map(|(enabled, label)| enabled.then_some(label))
                        .collect::<Vec<_>>()
                        .join(", ");
                        ui.label(if attributes.is_empty() {
                            "—"
                        } else {
                            &attributes
                        });
                        ui.label(if role.connection_limit == -1 {
                            "Unlimited".to_string()
                        } else {
                            role.connection_limit.to_string()
                        });
                        ui.label(role.valid_until.as_deref().unwrap_or("Never"));

                        let is_system_role = role.name.starts_with("pg_");
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!is_busy && !is_system_role, egui::Button::new("Edit"))
                                .clicked()
                            {
                                self.form = Some(RoleForm::from_role(role));
                            }
                            if ui
                                .add_enabled(
                                    !is_busy && !is_system_role,
                                    egui::Button::new(
                                        RichText::new("Drop").color(Color32::LIGHT_RED),
                                    ),
                                )
                                .on_hover_text(if is_system_role {
                                    "Built-in PostgreSQL roles cannot be changed here"
                                } else {
                                    "Drop this role"
                                })
                                .clicked()
                            {
                                action = Some(RolesAction::Drop(role.name.clone()));
                            }
                        });
                        ui.end_row();
                    }
                });
            if roles.is_empty() && !is_busy {
                ui.add_space(12.0);
                ui.label("No roles loaded. Use Refresh to load roles from the connected server.");
            }
        });

        if let Some(form) = &mut self.form {
            let is_edit = form.original_name.is_some();
            let mut keep_open = true;
            let mut request_close = false;
            let mut save_form = None;
            Window::new(if is_edit {
                "Edit PostgreSQL Role"
            } else {
                "Create PostgreSQL Role"
            })
            .collapsible(false)
            .resizable(true)
            .open(&mut keep_open)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(440.0);
                ui.label("Role name");
                ui.add_enabled(
                    !is_busy,
                    egui::TextEdit::singleline(&mut form.name).desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                ui.label(if is_edit {
                    "New password (leave blank to keep the current password)"
                } else {
                    "Password (optional)"
                });
                ui.add_enabled(
                    !is_busy,
                    egui::TextEdit::singleline(&mut form.password)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);

                ui.columns(2, |columns| {
                    columns[0].checkbox(&mut form.can_login, "Can login (user)");
                    columns[0].checkbox(&mut form.is_superuser, "Superuser");
                    columns[0].checkbox(&mut form.can_create_db, "Create databases");
                    columns[0].checkbox(&mut form.can_create_role, "Create roles");
                    columns[1].checkbox(&mut form.can_inherit, "Inherit privileges");
                    columns[1].checkbox(&mut form.can_replicate, "Replication");
                    columns[1].checkbox(&mut form.can_bypass_rls, "Bypass row security");
                });
                ui.add_space(6.0);

                egui::CollapsingHeader::new(format!("Membership ({})", form.memberships.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.label("Roles this role is a member of:");
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for available_role in roles.iter().filter(|role| {
                                    form.original_name.as_deref() != Some(role.name.as_str())
                                        && form.name.trim() != role.name
                                }) {
                                    let mut selected =
                                        form.memberships.contains(&available_role.name);
                                    if ui.checkbox(&mut selected, &available_role.name).changed() {
                                        if selected {
                                            form.memberships.push(available_role.name.clone());
                                        } else {
                                            form.memberships
                                                .retain(|name| name != &available_role.name);
                                        }
                                    }
                                }
                            });
                    });
                ui.add_space(6.0);

                egui::Grid::new("role_options_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Connection limit:");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.connection_limit)
                                .desired_width(100.0),
                        );
                        ui.end_row();
                        ui.label("Valid until:");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.valid_until)
                                .hint_text("blank = never")
                                .desired_width(240.0),
                        );
                        ui.end_row();
                    });
                ui.label(
                    RichText::new("Use a PostgreSQL timestamp such as 2027-01-31 23:59:59+00.")
                        .size(11.0)
                        .color(Color32::GRAY),
                );

                let mut preview_form = form.clone();
                if !preview_form.password.is_empty() {
                    preview_form.password = "<hidden>".to_string();
                }
                let preview = crate::db::role_sql_preview(&preview_form);
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Generated SQL preview (password hidden)")
                        .size(11.0)
                        .color(Color32::GRAY),
                );
                egui::Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                    .rounding(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        let mut preview_text =
                            preview.unwrap_or_else(|error| format!("Cannot generate SQL: {error}"));
                        ui.add(
                            egui::TextEdit::multiline(&mut preview_text)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        request_close = true;
                    }
                    if ui
                        .add_enabled(
                            !is_busy && !form.name.trim().is_empty(),
                            egui::Button::new(if is_edit {
                                "Save Changes"
                            } else {
                                "Create Role"
                            }),
                        )
                        .clicked()
                    {
                        save_form = Some(form.clone());
                        request_close = true;
                    }
                });
            });
            if request_close {
                keep_open = false;
            }
            if !keep_open || save_form.is_some() {
                self.form = None;
            }
            if let Some(form) = save_form {
                action = Some(RolesAction::Save(form));
            }
        }

        action
    }
}
