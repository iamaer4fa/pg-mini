use crate::db::introspect::ColumnMeta;
use crate::models::ImportCsvProgress;
use crate::ui::data_table::DataTableAction;
use egui::{Color32, RichText, Ui};
use std::path::PathBuf;

/// CSV Import Wizard state
#[derive(Debug, Clone, Default)]
pub struct CsvImportWizard {
    /// File path selected by user
    pub file_path: Option<PathBuf>,
    /// Column metadata from database
    pub column_meta: Vec<ColumnMeta>,
    /// Preview rows (first 10 rows from CSV)
    pub preview_rows: Vec<Vec<String>>,
    /// CSV headers
    pub headers: Vec<String>,
    /// Selected columns to import (by index)
    pub selected_columns: Vec<bool>,
    /// Current step: 0 = file selection, 1 = preview, 2 = importing
    pub step: usize,
    /// Import progress
    pub import_progress: Option<(usize, usize)>,
    /// Error message if any
    pub import_error: Option<String>,
    /// Whether import is in progress
    pub importing: bool,
    /// Total rows in CSV (estimated)
    pub total_rows: usize,
    /// Column mapping: CSV column index -> DB column index
    pub column_mapping: Vec<Option<usize>>,
    /// Estimated file size
    pub file_size: u64,
}

impl CsvImportWizard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset wizard to initial state
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Check if we can proceed to next step
    pub fn can_proceed(&self) -> bool {
        match self.step {
            0 => self.file_path.is_some(),
            1 => !self.selected_columns.is_empty() && self.selected_columns.iter().any(|&s| s),
            _ => false,
        }
    }

    /// Render the CSV import wizard UI
    pub fn show(&mut self, ui: &mut Ui, table_schema: &[ColumnMeta], schema: &str, table: &str) -> Option<DataTableAction> {
        let mut action = None;

        match self.step {
            0 => {
                self.show_file_selection(ui, table_schema, schema, table, &mut action);
            }
            1 => {
                self.show_preview(ui, table_schema, schema, table, &mut action);
            }
            2 => {
                self.show_importing(ui);
            }
            _ => {}
        }

        action
    }

    fn show_file_selection(&mut self, ui: &mut Ui, _table_schema: &[ColumnMeta], _schema: &str, _table: &str, _action: &mut Option<DataTableAction>) {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(RichText::new("📥 Import CSV").strong().size(20.0));
            ui.add_space(10.0);
            ui.label("Select a CSV file to import into the current table.");
            ui.add_space(20.0);

            if ui.button("📁 Choose CSV File").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV Files", &["csv"])
                    .pick_file()
                {
                    self.file_path = Some(path.clone());
                    // Get file size
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        self.file_size = metadata.len();
                    }
                }
            }

            if let Some(path) = &self.file_path {
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Selected: {}", path.display())).color(Color32::LIGHT_GRAY));
                ui.add_space(10.0);
                if ui.button("Next: Preview →").clicked() {
                    self.step = 1;
                }
            }
        });
    }

    fn show_preview(&mut self, ui: &mut Ui, table_schema: &[ColumnMeta], schema: &str, table: &str, action: &mut Option<DataTableAction>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("📥 Import CSV").strong().size(18.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("← Back").clicked() {
                    self.step = 0;
                }
            });
        });
        ui.separator();

        // Load preview if not already loaded
        if self.preview_rows.is_empty() && self.file_path.is_some() {
            self.load_preview();
        }

        if self.preview_rows.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label("Loading preview...");
            });
            return;
        }

        ui.label(RichText::new("Preview (first 10 rows):").strong());
        ui.add_space(5.0);

        // Column mapping table
        egui::ScrollArea::both().max_height(300.0).show(ui, |ui| {
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(egui_extras::Column::auto().at_least(40.0))   // Checkbox
                .column(egui_extras::Column::auto().at_least(150.0))  // CSV Column
                .column(egui_extras::Column::auto().at_least(150.0))  // DB Column
                .column(egui_extras::Column::auto().at_least(100.0))  // Type
                .header(24.0, |mut header| {
                    header.col(|ui| { ui.strong("Import"); });
                    header.col(|ui| { ui.strong("CSV Column"); });
                    header.col(|ui| { ui.strong("Map to DB Column"); });
                    header.col(|ui| { ui.strong("Type"); });
                })
                .body(|mut body| {
                    for (csv_idx, header_name) in self.headers.iter().enumerate() {
                        body.row(24.0, |mut row| {
                            // Import checkbox
                            row.col(|ui| {
                                let mut selected = self.selected_columns.get(csv_idx).copied().unwrap_or(true);
                                if ui.checkbox(&mut selected, "").clicked() {
                                    if self.selected_columns.len() <= csv_idx {
                                        self.selected_columns.resize(self.headers.len(), true);
                                    }
                                    self.selected_columns[csv_idx] = selected;
                                }
                            });

                            // CSV column name
                            row.col(|ui| {
                                ui.label(RichText::new(header_name).monospace());
                            });

                            // DB column mapping dropdown
                            row.col(|ui| {
                                let current_mapping = self.column_mapping.get(csv_idx).and_then(|m| *m);
                                egui::ComboBox::from_id_salt(format!("col_map_{}", csv_idx))
                                    .width(180.0)
                                    .selected_text(
                                        current_mapping
                                            .map(|idx| table_schema.get(idx).map(|c| c.name.as_str()).unwrap_or(""))
                                            .unwrap_or("-- Skip --")
                                    )
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.column_mapping[csv_idx], None, "-- Skip --");
                                        for (db_idx, col_meta) in table_schema.iter().enumerate() {
                                            let is_mapped = self.column_mapping.iter().any(|m| m == &Some(db_idx));
                                            if !is_mapped || Some(db_idx) == current_mapping {
                                                ui.selectable_value(&mut self.column_mapping[csv_idx], Some(db_idx), &col_meta.name);
                                            }
                                        }
                                    });
                            });

                            // Target column type
                            row.col(|ui| {
                                if let Some(Some(db_idx)) = self.column_mapping.get(csv_idx) {
                                    if let Some(meta) = table_schema.get(*db_idx) {
                                        ui.label(RichText::new(&meta.pg_type).color(Color32::LIGHT_BLUE).monospace());
                                    }
                                } else {
                                    ui.label(RichText::new("—").color(Color32::GRAY));
                                }
                            });
                        });
                    }
                });
        });

        ui.add_space(10.0);
        ui.separator();

        // Preview data table
        ui.label(RichText::new("Data Preview:").strong());
        egui::ScrollArea::both().max_height(200.0).show(ui, |ui| {
            if !self.preview_rows.is_empty() {
                egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::auto().at_least(40.0))
                    .columns(egui_extras::Column::auto().at_least(120.0), self.headers.len())
                    .header(24.0, |mut header| {
                        header.col(|ui| { ui.strong("#"); });
                        for h in &self.headers {
                            header.col(|ui| { ui.strong(h); });
                        }
                    })
                    .body(|mut body| {
                        for (row_idx, data_row) in self.preview_rows.iter().enumerate() {
                            body.row(22.0, |mut table_row| {
                                table_row.col(|ui| { ui.label(format!("{}", row_idx + 1)); });
                                for cell in data_row {
                                    table_row.col(|ui| { ui.label(RichText::new(cell).monospace().size(11.0)); });
                                }
                            });
                        }
                    });
            }
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.step = 0;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.can_proceed(), egui::Button::new("Import →")).clicked() {
                    // Build the import action
                    if let Some(path) = &self.file_path {
                        let columns: Vec<String> = self.column_mapping
                            .iter()
                            .enumerate()
                            .filter_map(|(csv_idx, db_idx_opt)| {
                                if self.selected_columns.get(csv_idx).copied().unwrap_or(false) {
                                    db_idx_opt.map(|db_idx| table_schema[db_idx].name.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !columns.is_empty() {
                            // Decide between COPY and chunked INSERT based on file size and estimated rows
                            const COPY_THRESHOLD_ROWS: usize = 10_000;
                            const COPY_THRESHOLD_BYTES: u64 = 50 * 1024 * 1024; // 50 MB

                            // Estimate total rows from file size (rough estimate)
                            let estimated_rows = if self.total_rows > 0 {
                                self.total_rows
                            } else if self.file_size > 0 && !self.preview_rows.is_empty() {
                                // Estimate based on preview: file_size / avg_row_size
                                let avg_row_size = self.file_size / self.preview_rows.len().max(1) as u64;
                                (self.file_size / avg_row_size.max(1)) as usize
                            } else {
                                0
                            };

                            if estimated_rows > COPY_THRESHOLD_ROWS || self.file_size > COPY_THRESHOLD_BYTES {
                                *action = Some(DataTableAction::ImportCsvCopy {
                                    schema: schema.to_string(),
                                    table: table.to_string(),
                                    columns,
                                    file_path: path.clone(),
                                    total_rows: estimated_rows.max(1),
                                });
                            } else {
                                // For smaller files, we'd need to parse and send rows
                                // For now, fall back to a placeholder - the UI would need to parse the CSV
                                *action = Some(DataTableAction::ImportCsv {
                                    columns,
                                    rows: vec![], // Would need to parse full CSV here
                                    total_rows: estimated_rows.max(1),
                                });
                            }
                            self.step = 2;
                            self.importing = true;
                        }
                    }
                }
            });
        });
    }

    fn show_importing(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("📥 Importing CSV...").strong().size(18.0));
        });
        ui.separator();

        // Progress display
        if let Some((processed, total)) = self.import_progress {
            ui.add_space(20.0);
            ui.label(RichText::new(format!("Progress: {} / {} rows processed", processed, total)).strong().size(16.0));
            ui.add_space(10.0);
            let progress = if total > 0 { processed as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(progress).show_percentage());
            ui.add_space(10.0);

            if self.importing {
                ui.label(RichText::new("Import in progress... please wait").color(Color32::YELLOW));
            }
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label("Preparing import...");
            });
        }

        if let Some(err) = &self.import_error {
            ui.add_space(10.0);
            ui.label(RichText::new(format!("Error: {}", err)).color(Color32::RED).strong());
        }

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            if self.importing {
                ui.label(RichText::new("Import running... (cannot cancel from UI)").color(Color32::YELLOW));
            } else if self.import_error.is_some() {
                if ui.button("Close").clicked() {
                    self.reset();
                }
            } else {
                if ui.button("Done").clicked() {
                    self.reset();
                }
            }
        });
    }

    fn load_preview(&mut self) {
        if let Some(path) = &self.file_path {
            match self.parse_csv_preview(path) {
                Ok((headers, rows)) => {
                    self.headers = headers.clone();
                    self.preview_rows = rows;
                    self.selected_columns = vec![true; headers.len()];
                    self.column_mapping = vec![None; headers.len()];

                    // Auto-map columns by name
                    // This would be enhanced with actual table schema
                }
                Err(e) => {
                    self.import_error = Some(format!("Failed to parse CSV: {}", e));
                }
            }
        }
    }

    fn parse_csv_preview(&self, path: &PathBuf) -> Result<(Vec<String>, Vec<Vec<String>>), Box<dyn std::error::Error>> {
        use csv::ReaderBuilder;
        let mut rdr = ReaderBuilder::new()
            .flexible(true)
            .has_headers(true)
            .from_path(path)?;

        let headers = rdr.headers()?.clone().into_iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut rows = Vec::new();

        for (i, result) in rdr.records().enumerate() {
            if i >= 10 { break; }
            let record = result?;
            rows.push(record.iter().map(|s| s.to_string()).collect());
        }

        Ok((headers, rows))
    }
}

/// Simple CSV import progress widget for inline display
pub fn show_import_progress(ui: &mut Ui, progress: &ImportCsvProgress) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("📥 Importing:").color(Color32::YELLOW));
        ui.label(format!("{} / {} rows", progress.processed, progress.total));
        let pct = if progress.total > 0 { progress.processed as f32 / progress.total as f32 } else { 0.0 };
        ui.add(egui::ProgressBar::new(pct).show_percentage().desired_width(150.0));
        if progress.done {
            if let Some(err) = &progress.error {
                ui.label(RichText::new(format!("Error: {}", err)).color(Color32::RED));
            } else {
                ui.label(RichText::new("✓ Done").color(Color32::GREEN));
            }
        }
    });
}