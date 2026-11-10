use crate::ha::provisioning::{HaProvisioningWizard, ProvisioningPreflight, generate_setup_plan};
use crate::models::ConnectionConfig;
use egui::{Color32, RichText, Ui};
use std::collections::HashSet;

pub fn render_ha_provisioning(
    ui: &mut Ui,
    wizard: &mut HaProvisioningWizard,
    saved_connections: &[ConnectionConfig],
    connected_ids: &HashSet<String>,
    is_busy: bool,
) -> Option<Vec<String>> {
    let mut preflight_request = None;
    ui.collapsing("Guided physical replication setup (PostgreSQL 17/18)", |ui| {
        ui.label(
            RichText::new(
                "Guided only: pg-mini checks connected servers and generates instructions. It does not execute setup SQL, SSH to hosts, modify files, control services, or change data directories.",
            )
            .small()
            .color(Color32::GRAY),
        );
        ui.label(
            RichText::new(
                "Physical streaming replication replicates the entire PostgreSQL cluster (all databases), not one selected database.",
            )
            .small()
            .color(Color32::YELLOW),
        );
        if saved_connections.len() < 2 {
            ui.label("Save at least two distinct PostgreSQL connection profiles to begin.");
            return;
        }

        if wizard.primary_connection_id.is_none() {
            wizard.primary_connection_id = saved_connections.first().map(|profile| profile.id.clone());
        }
        if wizard.standby_connection_id.is_none() {
            wizard.standby_connection_id = saved_connections
                .iter()
                .find(|profile| Some(&profile.id) != wizard.primary_connection_id.as_ref())
                .map(|profile| profile.id.clone());
        }

        ui.horizontal(|ui| {
            ui.label("Primary profile");
            let selected_name = profile_name(saved_connections, wizard.primary_connection_id.as_deref());
            egui::ComboBox::from_id_salt("ha_provisioning_primary")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for profile in saved_connections {
                        ui.selectable_value(
                            &mut wizard.primary_connection_id,
                            Some(profile.id.clone()),
                            format!("{} ({}:{})", profile.name, profile.host, profile.port),
                        );
                    }
                });
            ui.label(connection_state(
                wizard.primary_connection_id.as_deref(),
                connected_ids,
                &wizard.pending_preflights,
                &wizard.preflights,
            ));
        });
        ui.horizontal(|ui| {
            ui.label("Standby profile");
            let selected_name = profile_name(saved_connections, wizard.standby_connection_id.as_deref());
            egui::ComboBox::from_id_salt("ha_provisioning_standby")
                .selected_text(selected_name)
                .show_ui(ui, |ui| {
                    for profile in saved_connections {
                        ui.selectable_value(
                            &mut wizard.standby_connection_id,
                            Some(profile.id.clone()),
                            format!("{} ({}:{})", profile.name, profile.host, profile.port),
                        );
                    }
                });
            ui.label(connection_state(
                wizard.standby_connection_id.as_deref(),
                connected_ids,
                &wizard.pending_preflights,
                &wizard.preflights,
            ));
        });

        ui.horizontal(|ui| {
            ui.label("Replication role");
            ui.add(
                egui::TextEdit::singleline(&mut wizard.replication_role)
                    .desired_width(180.0),
            );
            ui.label("Standby source CIDR");
            ui.add(
                egui::TextEdit::singleline(&mut wizard.allowed_cidr)
                    .hint_text("192.0.2.10/32")
                    .desired_width(160.0),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut wizard.use_physical_slot,
                "Use a physical replication slot",
            );
            if wizard.use_physical_slot {
                ui.label("Slot name");
                ui.add(
                    egui::TextEdit::singleline(&mut wizard.slot_name)
                        .desired_width(180.0),
                );
            }
        });

        let pair_ids = match (
            wizard.primary_connection_id.as_ref(),
            wizard.standby_connection_id.as_ref(),
        ) {
            (Some(primary), Some(standby)) if primary != standby => {
                Some(vec![primary.clone(), standby.clone()])
            }
            _ => None,
        };
        let can_check = pair_ids.as_ref().is_some_and(|ids| {
            ids.iter().all(|id| connected_ids.contains(id))
                && ids
                    .iter()
                    .all(|id| !wizard.pending_preflights.contains(id))
        });
        if ui
            .add_enabled(!is_busy && can_check, egui::Button::new("Run read-only preflight"))
            .on_hover_text("Both selected saved profiles must currently be connected.")
            .clicked()
        {
            preflight_request = pair_ids;
        }
        if !can_check {
            ui.label(
                RichText::new("Connect both selected saved profiles before running preflight.")
                    .small()
                    .color(Color32::GRAY),
            );
        }

        for id in wizard
            .primary_connection_id
            .iter()
            .chain(wizard.standby_connection_id.iter())
        {
            if let Some(Ok(preflight)) = wizard.preflights.get(id) {
                render_preflight(ui, id, preflight, saved_connections);
            } else if let Some(Err(error)) = wizard.preflights.get(id) {
                ui.label(RichText::new(error).color(Color32::LIGHT_RED));
            } else if wizard.pending_preflights.contains(id) {
                ui.label(RichText::new("Running read-only checks...").color(Color32::GRAY));
            }
        }

        let selected = selected_profiles(wizard, saved_connections);
        if let Some((primary, standby)) = selected {
            if let (Some(Ok(primary_preflight)), Some(Ok(standby_preflight))) = (
                wizard.preflights.get(&primary.id),
                wizard.preflights.get(&standby.id),
            ) {
                match generate_setup_plan(
                    primary,
                    standby,
                    primary_preflight,
                    standby_preflight,
                    &wizard.replication_role,
                    &wizard.allowed_cidr,
                    wizard.use_physical_slot,
                    &wizard.slot_name,
                ) {
                    Ok(plan) => {
                        render_setup_plan(ui, primary_preflight, &plan);
                        render_verification(ui, primary_preflight, standby_preflight);
                    }
                    Err(error) => {
                        ui.label(RichText::new(error).color(Color32::LIGHT_RED));
                    }
                }
            }
        }
    });
    preflight_request
}

fn profile_name<'a>(profiles: &'a [ConnectionConfig], selected_id: Option<&str>) -> &'a str {
    selected_id
        .and_then(|id| profiles.iter().find(|profile| profile.id == id))
        .map_or("Select a saved connection", |profile| profile.name.as_str())
}

fn selected_profiles<'a>(
    wizard: &HaProvisioningWizard,
    profiles: &'a [ConnectionConfig],
) -> Option<(&'a ConnectionConfig, &'a ConnectionConfig)> {
    let primary = profiles
        .iter()
        .find(|profile| Some(&profile.id) == wizard.primary_connection_id.as_ref())?;
    let standby = profiles
        .iter()
        .find(|profile| Some(&profile.id) == wizard.standby_connection_id.as_ref())?;
    Some((primary, standby))
}

fn connection_state(
    selected_id: Option<&str>,
    connected_ids: &HashSet<String>,
    pending: &HashSet<String>,
    results: &std::collections::HashMap<String, Result<ProvisioningPreflight, String>>,
) -> RichText {
    let Some(id) = selected_id else {
        return RichText::new("Not selected").color(Color32::GRAY);
    };
    if pending.contains(id) {
        RichText::new("Checking...").color(Color32::YELLOW)
    } else if let Some(Err(_)) = results.get(id) {
        RichText::new("Check failed").color(Color32::LIGHT_RED)
    } else if results.contains_key(id) {
        RichText::new("Checked").color(Color32::LIGHT_GREEN)
    } else if connected_ids.contains(id) {
        RichText::new("Connected; preflight needed").color(Color32::YELLOW)
    } else {
        RichText::new("Not connected").color(Color32::GRAY)
    }
}

fn render_preflight(
    ui: &mut Ui,
    connection_id: &str,
    preflight: &ProvisioningPreflight,
    profiles: &[ConnectionConfig],
) {
    let name = profiles
        .iter()
        .find(|profile| profile.id == connection_id)
        .map_or(connection_id, |profile| profile.name.as_str());
    ui.group(|ui| {
        ui.label(RichText::new(format!("{name}: {}", preflight.server_version)).strong());
        ui.label(format!(
            "Current role: {}; supported for provisioning: {}; wal_level: {}; senders: {}; slots: {}; SSL: {}",
            if preflight.is_primary() {
                "primary"
            } else {
                "in recovery"
            },
            if preflight.supports_provisioning() {
                "yes"
            } else {
                "no (17/18 only)"
            },
            preflight.wal_level,
            preflight.max_wal_senders,
            preflight.max_replication_slots,
            if preflight.ssl_enabled { "on" } else { "off" }
        ));
        ui.label(format!(
            "Connected account: superuser={}, CREATEROLE={}, REPLICATION={}",
            preflight.is_superuser, preflight.can_create_role, preflight.can_replication
        ));
        if let Some(system_identifier) = &preflight.system_identifier {
            ui.label(format!(
                "System identifier: {}",
                system_identifier
            ));
        } else if let Some(error) = &preflight.system_identifier_error {
            ui.label(
                RichText::new(format!("System identifier unknown: {error}"))
                    .small()
                    .color(Color32::GRAY),
            );
        }
        if !preflight.is_in_recovery && preflight.system_identifier.is_some() {
            ui.label(
                RichText::new(
                    "The target is a running PostgreSQL instance. The administrator must choose a separate empty standby data directory and handle any existing data manually.",
                )
                .color(Color32::YELLOW)
                .small(),
            );
        }
    });
}

fn render_setup_plan(
    ui: &mut Ui,
    primary_preflight: &ProvisioningPreflight,
    plan: &crate::ha::provisioning::ReplicationSetupPlan,
) {
    ui.separator();
    ui.label(RichText::new("Reviewable setup plan").strong().size(16.0));
    ui.label(format!(
        "Provisioning target: PostgreSQL {}. Primary system identifier: {}",
        primary_preflight.version_label(),
        primary_preflight
            .system_identifier
            .as_deref()
            .unwrap_or("unknown; not required for plan generation")
    ));

    ui.label(RichText::new("1. Primary — create role").strong());
    ui.label(
        RichText::new("Apply the role SQL as a PostgreSQL superuser; first check that this role name does not already exist. Then set the secret interactively in psql. The password is never included in generated output.")
            .small()
            .color(Color32::GRAY),
    );
    copyable_text(ui, &plan.primary_role_sql, 65.0);
    ui.label("In psql, set a password without putting it in SQL history:");
    copyable_text(ui, &plan.set_password_command, 38.0);

    ui.label(RichText::new("2. Primary — review settings and network").strong());
    copyable_text(ui, &plan.primary_settings, 90.0);
    ui.label("Append this restricted rule to pg_hba.conf, review it, then reload PostgreSQL:");
    copyable_text(ui, &plan.pg_hba_rule, 38.0);

    ui.label(RichText::new("3. Standby — bootstrap manually").strong());
    ui.colored_label(
        Color32::YELLOW,
        "DATA DIRECTORY WARNING: pg_basebackup requires an empty target directory. Back up or move any existing data yourself, and stop the target PostgreSQL service before replacing its data.",
    );
    copyable_text(ui, &plan.standby_command, 155.0);
    if let Some(warning) = &plan.slot_warning {
        ui.colored_label(Color32::YELLOW, warning);
    }
    ui.label(
        RichText::new(
            "No command or SQL above has been run. Verify service paths, network rules, backups, TLS certificates, and permissions for your servers before applying the plan.",
        )
        .small()
        .color(Color32::GRAY),
    );
}

fn render_verification(
    ui: &mut Ui,
    primary: &ProvisioningPreflight,
    standby: &ProvisioningPreflight,
) {
    ui.separator();
    ui.label(RichText::new("4. Verify after setup").strong());
    ui.label(
        "After applying the plan and starting the standby, run the preflight again. Confirm the primary remains out of recovery, the standby is in recovery, and both system identifiers match. HA Overview also reports live WAL receiver and primary replication status.",
    );
    if !primary.is_primary() {
        ui.colored_label(
            Color32::LIGHT_RED,
            "Selected primary is currently in recovery.",
        );
    }
    if standby.is_in_recovery {
        ui.colored_label(Color32::LIGHT_GREEN, "Standby is in recovery.");
    } else {
        ui.colored_label(
            Color32::YELLOW,
            "Standby is not currently in recovery; this may be the pre-setup state.",
        );
    }
    match (&primary.system_identifier, &standby.system_identifier) {
        (Some(primary_id), Some(standby_id)) if primary_id == standby_id => {
            ui.colored_label(Color32::LIGHT_GREEN, "System identifiers match.");
        }
        (Some(_), Some(_)) => {
            ui.colored_label(Color32::LIGHT_RED, "System identifiers do not match.");
        }
        _ => {
            ui.colored_label(
                Color32::GRAY,
                "System identifier comparison is unknown because one or both accounts cannot read it.",
            );
        }
    }
}

fn copyable_text(ui: &mut Ui, text: &str, height: f32) {
    let mut displayed_text = text.to_string();
    ui.add(
        egui::TextEdit::multiline(&mut displayed_text)
            .font(egui::TextStyle::Monospace)
            .desired_width(ui.available_width())
            .desired_rows((height / 18.0).ceil() as usize)
            .interactive(false),
    );
    if ui.button("Copy").clicked() {
        ui.ctx()
            .output_mut(|output| output.copied_text = text.to_string());
    }
}
