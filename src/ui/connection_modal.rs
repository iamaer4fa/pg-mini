use crate::models::{AppConfig, ConnectionConfig, TlsMode};
use egui::{Color32, RichText, Window};

pub enum ConnectionModalAction {
    Connect {
        config: ConnectionConfig,
        save_error: Option<String>,
    },
    ImportProfiles,
    ExportProfiles {
        include_passwords: bool,
    },
}

pub struct ConnectionModal {
    pub is_open: bool,
    pub current_cfg: ConnectionConfig,
    pub show_password: bool,
    pub error_message: Option<String>,
    pub include_passwords_in_export: bool,
}

impl ConnectionModal {
    pub fn new(initial_cfg: ConnectionConfig) -> Self {
        Self {
            is_open: true,
            current_cfg: initial_cfg,
            show_password: false,
            error_message: None,
            include_passwords_in_export: false,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        config: &mut AppConfig,
        is_connected: bool,
        is_busy: bool,
    ) -> Option<ConnectionModalAction> {
        let mut action = None;
        if !self.is_open {
            return None;
        }

        Window::new("🔌 PostgreSQL Connection")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(480.0);

                ui.horizontal(|ui| {
                    ui.heading("Connect to PostgreSQL");
                    if is_connected {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✖ Close").clicked() {
                                self.is_open = false;
                            }
                        });
                    }
                });

                ui.separator();
                ui.add_space(6.0);

                // Profiles selector
                ui.horizontal(|ui| {
                    ui.label("Saved Profile:");
                    let mut selected_id = self.current_cfg.id.clone();
                    egui::ComboBox::from_id_salt("saved_profiles")
                        .width(220.0)
                        .selected_text(&self.current_cfg.name)
                        .show_ui(ui, |ui| {
                            for p in &config.saved_connections {
                                if ui
                                    .selectable_value(&mut selected_id, p.id.clone(), &p.name)
                                    .clicked()
                                {
                                    self.current_cfg = p.clone();
                                    self.error_message = None;
                                }
                            }
                        });

                    if ui
                        .button("➕ New")
                        .on_hover_text("Create a new connection profile")
                        .clicked()
                    {
                        let mut new_cfg = ConnectionConfig::default();
                        new_cfg.name = format!("Connection {}", config.saved_connections.len() + 1);
                        config.saved_connections.push(new_cfg.clone());
                        self.current_cfg = new_cfg;
                        self.error_message = None;
                    }

                    if config.saved_connections.len() > 1
                        && ui
                            .button("🗑 Delete")
                            .on_hover_text("Delete current profile")
                            .clicked()
                    {
                        config
                            .saved_connections
                            .retain(|p| p.id != self.current_cfg.id);
                        if let Some(first) = config.saved_connections.first() {
                            self.current_cfg = first.clone();
                        }
                    }
                });

                ui.add_space(8.0);
                egui::Grid::new("conn_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Profile Name:");
                        ui.text_edit_singleline(&mut self.current_cfg.name);
                        ui.end_row();

                        ui.label("Host:");
                        ui.text_edit_singleline(&mut self.current_cfg.host);
                        ui.end_row();

                        ui.label("Port:");
                        let mut port_str = self.current_cfg.port.to_string();
                        if ui.text_edit_singleline(&mut port_str).changed() {
                            if let Ok(p) = port_str.parse::<u16>() {
                                self.current_cfg.port = p;
                            }
                        }
                        ui.end_row();

                        ui.label("Database:");
                        ui.text_edit_singleline(&mut self.current_cfg.database);
                        ui.end_row();

                        ui.label("Username:");
                        ui.text_edit_singleline(&mut self.current_cfg.username);
                        ui.end_row();

                        ui.label("Password:");
                        ui.horizontal(|ui| {
                            if self.show_password {
                                ui.text_edit_singleline(&mut self.current_cfg.password);
                            } else {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.current_cfg.password)
                                        .password(true),
                                );
                            }
                            if ui
                                .button(if self.show_password {
                                    "👁 Hide"
                                } else {
                                    "👁 Show"
                                })
                                .clicked()
                            {
                                self.show_password = !self.show_password;
                            }
                        });
                        ui.end_row();

                        ui.label("TLS:");
                        egui::ComboBox::from_id_salt("connection_tls_mode")
                            .selected_text(self.current_cfg.tls_mode.label())
                            .show_ui(ui, |ui| {
                                for mode in [TlsMode::Disable, TlsMode::Prefer, TlsMode::Require] {
                                    ui.selectable_value(
                                        &mut self.current_cfg.tls_mode,
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                        ui.end_row();

                        if self.current_cfg.tls_mode != TlsMode::Disable {
                            ui.label("Additional CA certificate:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.current_cfg.tls_root_cert)
                                        .hint_text("Optional PEM file"),
                                );
                                if ui.button("Browse...").clicked() {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("PEM certificate", &["pem", "crt", "cer"])
                                        .pick_file()
                                    {
                                        self.current_cfg.tls_root_cert =
                                            path.to_string_lossy().into_owned();
                                    }
                                }
                            });
                            ui.end_row();
                        }

                        ui.label("Statement timeout:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(
                                    &mut self.current_cfg.statement_timeout_seconds,
                                )
                                .range(0..=86_400)
                                .suffix(" s"),
                            );
                            ui.label("0 = disabled");
                        });
                        ui.end_row();

                        ui.label("Lock timeout:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.current_cfg.lock_timeout_seconds)
                                    .range(0..=86_400)
                                    .suffix(" s"),
                            );
                            ui.label("0 = disabled");
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Import Profiles...").clicked() {
                        action = Some(ConnectionModalAction::ImportProfiles);
                    }
                    if ui.button("Export Profiles...").clicked() {
                        if let Some(pos) = config
                            .saved_connections
                            .iter()
                            .position(|profile| profile.id == self.current_cfg.id)
                        {
                            config.saved_connections[pos] = self.current_cfg.clone();
                        } else {
                            config.saved_connections.push(self.current_cfg.clone());
                        }
                        self.error_message = crate::config::ConfigManager::save(config)
                            .err()
                            .map(|error| format!("Could not save profile: {error}"));
                        action = Some(ConnectionModalAction::ExportProfiles {
                            include_passwords: self.include_passwords_in_export,
                        });
                    }
                    ui.checkbox(
                        &mut self.include_passwords_in_export,
                        "Include passwords (plaintext)",
                    );
                });

                if let Some(err) = &self.error_message {
                    ui.add_space(10.0);
                    egui::Frame::none()
                        .fill(Color32::from_rgb(60, 20, 20))
                        .stroke(egui::Stroke::new(1.0_f32, Color32::RED))
                        .inner_margin(8.0)
                        .rounding(4.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("⚠ Connection/Profile Error:")
                                        .color(Color32::LIGHT_RED)
                                        .strong(),
                                );
                            });
                            ui.label(
                                RichText::new(err)
                                    .color(Color32::from_rgb(255, 180, 180))
                                    .monospace(),
                            );
                        });
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    if ui.button("💾 Save Profile").clicked() {
                        if let Some(pos) = config
                            .saved_connections
                            .iter()
                            .position(|p| p.id == self.current_cfg.id)
                        {
                            config.saved_connections[pos] = self.current_cfg.clone();
                        } else {
                            config.saved_connections.push(self.current_cfg.clone());
                        }
                        self.error_message = crate::config::ConfigManager::save(config)
                            .err()
                            .map(|error| format!("Could not save profile: {error}"));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let connect_btn = ui.add_enabled(
                            !is_busy,
                            egui::Button::new(
                                RichText::new(if is_busy {
                                    "⏳ Connecting..."
                                } else {
                                    "🔌 Connect"
                                })
                                .strong(),
                            )
                            .fill(Color32::from_rgb(16, 145, 90)),
                        );

                        if connect_btn.clicked() {
                            // Update saved profiles
                            if let Some(pos) = config
                                .saved_connections
                                .iter()
                                .position(|p| p.id == self.current_cfg.id)
                            {
                                config.saved_connections[pos] = self.current_cfg.clone();
                            } else {
                                config.saved_connections.push(self.current_cfg.clone());
                            }
                            config.last_connection_id = Some(self.current_cfg.id.clone());
                            let save_error = crate::config::ConfigManager::save(config)
                                .err()
                                .map(|error| format!("Could not save profile: {error}"));
                            self.error_message = save_error.clone();
                            action = Some(ConnectionModalAction::Connect {
                                config: self.current_cfg.clone(),
                                save_error,
                            });
                        }

                        if is_connected && ui.button("Cancel").clicked() {
                            self.is_open = false;
                        }
                    });
                });
            });

        action
    }
}
