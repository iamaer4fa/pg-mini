use crate::models::BackupRestorePreflight;
use egui::{RichText, ScrollArea, Window};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

const MAX_PROGRESS_LINES: usize = 100;

#[derive(Clone)]
enum TaskKind {
    Backup,
    Restore,
}

#[derive(Clone)]
struct Task {
    kind: TaskKind,
    database: String,
    server: String,
    file_path: PathBuf,
}

pub enum BackupRestoreAction {
    RunBackup {
        database: String,
        file_path: PathBuf,
    },
    RunRestore {
        database: String,
        file_path: PathBuf,
        clean_before_restore: bool,
        selected_entries: Vec<String>,
    },
}

#[derive(Default)]
pub struct BackupRestoreDialog {
    task: Option<Task>,
    preflight: Option<Result<BackupRestorePreflight, String>>,
    completion: Option<Result<String, String>>,
    progress: VecDeque<String>,
    selected_entries: HashSet<usize>,
    clean_before_restore: bool,
    started_at: Option<Instant>,
}

impl BackupRestoreDialog {
    pub fn is_open(&self) -> bool {
        self.task.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn begin_backup(&mut self, database: String, server: String, file_path: PathBuf) {
        self.begin(TaskKind::Backup, database, server, file_path);
    }

    pub fn begin_restore(&mut self, database: String, server: String, file_path: PathBuf) {
        self.begin(TaskKind::Restore, database, server, file_path);
    }

    fn begin(&mut self, kind: TaskKind, database: String, server: String, file_path: PathBuf) {
        self.task = Some(Task {
            kind,
            database,
            server,
            file_path,
        });
        self.preflight = None;
        self.completion = None;
        self.progress.clear();
        self.selected_entries.clear();
        self.clean_before_restore = false;
        self.started_at = None;
    }

    pub fn set_preflight(&mut self, result: Result<BackupRestorePreflight, String>) {
        if let Ok(preflight) = &result {
            self.selected_entries = (0..preflight.restore_entries.len()).collect();
        }
        self.preflight = Some(result);
    }

    pub fn add_progress(&mut self, message: String) {
        if !message.trim().is_empty() {
            self.progress.push_back(message);
            while self.progress.len() > MAX_PROGRESS_LINES {
                self.progress.pop_front();
            }
        }
    }

    pub fn finish(&mut self, result: Result<String, String>) {
        self.started_at = None;
        self.completion = Some(result);
    }

    pub fn show(&mut self, ctx: &egui::Context, is_busy: bool) -> Option<BackupRestoreAction> {
        let task = self.task.clone()?;
        let title = match task.kind {
            TaskKind::Backup => "Database Backup",
            TaskKind::Restore => "Database Restore",
        };
        let mut action = None;
        let mut close = false;
        let preflight = self.preflight.clone();
        let completion = self.completion.clone();
        let entries = preflight
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|details| details.restore_entries.clone())
            .unwrap_or_default();

        Window::new(title)
            .collapsible(false)
            .resizable([true, true])
            .default_size([640.0, 560.0])
            .show(ctx, |ui| {
                let content_max_height = (ui.available_height() - 48.0).max(120.0);
                ScrollArea::vertical()
                    .id_salt("backup_restore_dialog_content")
                    .max_height(content_max_height)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Target: {} on {}    File: {}",
                                task.database,
                                task.server,
                                task.file_path.display()
                            ))
                            .strong(),
                        );
                        ui.separator();

                        match &preflight {
                            None => {
                                ui.label(
                                    "Checking client tools, target database, and destination...",
                                );
                            }
                            Some(Err(error)) => {
                                render_error_message(
                                    ui,
                                    "backup_restore_preflight_error",
                                    error,
                                );
                            }
                            Some(Ok(details)) => {
                        ui.label(format!(
                            "Tool: {} ({})    Server: {}    Format: {}",
                            details.tool,
                            details.tool_version,
                            details.server_version,
                            details.format
                        ));
                        if let Some(bytes) = details.database_size_bytes {
                            ui.label(format!("Database size: {}", format_bytes(bytes)));
                        } else {
                            ui.label("Database size estimate: unavailable");
                        }
                        if let Some(bytes) = details.available_space_bytes {
                            ui.label(format!("Destination free space: {}", format_bytes(bytes)));
                        } else if matches!(task.kind, TaskKind::Backup) {
                            ui.label("Destination free space: unavailable on this platform");
                        } else {
                            ui.label(
                                "Target server free space cannot be queried by pg-mini; verify it on the PostgreSQL host.",
                            );
                        }
                        ui.label(
                            RichText::new(&details.command_preview)
                                .monospace()
                                .small(),
                        );
                        for warning in &details.warnings {
                            ui.colored_label(egui::Color32::YELLOW, warning);
                        }

                        if matches!(task.kind, TaskKind::Restore) {
                            if !details.restore_entries.is_empty() {
                                ui.checkbox(
                                    &mut self.clean_before_restore,
                                    "Clean existing objects before restoring (drops selected objects first)",
                                );
                                ui.horizontal(|ui| {
                                    if ui.small_button("Select all").clicked() {
                                        self.selected_entries = (0..entries.len()).collect();
                                    }
                                    if ui.small_button("Select none").clicked() {
                                        self.selected_entries.clear();
                                    }
                                    ui.label(format!(
                                        "{} of {} archive entries selected",
                                        self.selected_entries.len(),
                                        entries.len()
                                    ));
                                });
                                let row_height = ui
                                    .spacing()
                                    .interact_size
                                    .y
                                    .max(ui.text_style_height(&egui::TextStyle::Body));
                                ScrollArea::vertical()
                                    .max_height(180.0)
                                    .show_rows(ui, row_height, entries.len(), |ui, row_range| {
                                        for index in row_range {
                                            let entry = &entries[index];
                                            let mut selected =
                                                self.selected_entries.contains(&index);
                                            ui.horizontal(|ui| {
                                                if ui.checkbox(&mut selected, "").changed() {
                                                    if selected {
                                                        self.selected_entries.insert(index);
                                                    } else {
                                                        self.selected_entries.remove(&index);
                                                    }
                                                }
                                                ui.label(
                                                    RichText::new(short_entry_label(&entry.line))
                                                        .monospace(),
                                                )
                                                .on_hover_text(&entry.line);
                                            });
                                        }
                                    });
                            } else {
                                ui.label(
                                    "Plain SQL scripts do not support object selection or clean-before-restore.",
                                );
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "A failed SQL restore can leave partial changes in the target database.",
                                );
                            }
                            ui.label(
                                    RichText::new(if self.clean_before_restore {
                                        "Effect: selected existing objects will be dropped before their replacements are restored. A failed restore can leave partial changes."
                                    } else {
                                        "Effect: selected objects will be restored; existing conflicting objects can make the restore fail. A failed restore can leave partial changes."
                                    })
                                    .small()
                                    .color(egui::Color32::YELLOW),
                            );
                        }
                            }
                        }

                        if self.started_at.is_some() {
                            let elapsed = self.started_at.unwrap().elapsed().as_secs();
                            ui.label(format!("Running — elapsed {elapsed}s"));
                        }
                        if let Some(result) = &completion {
                            match result {
                                Ok(message) => {
                                    ui.colored_label(egui::Color32::LIGHT_GREEN, message);
                                }
                                Err(error) => {
                                    render_error_message(
                                        ui,
                                        "backup_restore_completion_error",
                                        error,
                                    );
                                }
                            }
                        }
                        if !self.progress.is_empty() {
                            ui.label("Tool output:");
                            ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                            for line in &self.progress {
                                    ui.label(RichText::new(line).monospace());
                                }
                            }
                            );
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if self.started_at.is_none() && self.preflight.is_some() {
                        if ui.button(if completion.is_some() { "Close" } else { "Cancel" }).clicked() {
                            close = true;
                        }
                    }
                    let can_run = !is_busy
                        && self.started_at.is_none()
                        && completion.is_none()
                        && preflight.as_ref().is_some_and(Result::is_ok)
                        && (!matches!(task.kind, TaskKind::Restore)
                            || entries.is_empty()
                            || !self.selected_entries.is_empty());
                    if ui
                        .add_enabled(
                            can_run,
                            egui::Button::new(match task.kind {
                                TaskKind::Backup => "Start Backup",
                                TaskKind::Restore => "Confirm and Restore",
                            }),
                        )
                        .clicked()
                    {
                        action = Some(match task.kind {
                            TaskKind::Backup => BackupRestoreAction::RunBackup {
                                database: task.database.clone(),
                                file_path: task.file_path.clone(),
                            },
                            TaskKind::Restore => BackupRestoreAction::RunRestore {
                                database: task.database.clone(),
                                file_path: task.file_path.clone(),
                                clean_before_restore: self.clean_before_restore,
                                selected_entries: entries
                                    .iter()
                                    .enumerate()
                                    .filter(|(index, _)| self.selected_entries.contains(index))
                                    .map(|(_, entry)| entry.line.clone())
                                    .collect(),
                            },
                        });
                        self.started_at = Some(Instant::now());
                    }
                });
            });

        if close {
            self.task = None;
        }
        if self.started_at.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        action
    }
}

fn render_error_message(ui: &mut egui::Ui, id: &'static str, error: &str) {
    ScrollArea::both()
        .id_salt(id)
        .max_width(ui.available_width())
        .max_height(140.0)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(error)
                        .monospace()
                        .color(egui::Color32::LIGHT_RED),
                )
                .extend(),
            );
        });
}

fn short_entry_label(line: &str) -> String {
    let mut chars = line.chars();
    let preview: String = chars.by_ref().take(56).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
