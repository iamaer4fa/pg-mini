use crate::models::{CellValue, EditableCell, QueryResult};
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

pub struct DataTableState {
    pub search_query: String,
    pub page_limit: usize,
    pub current_page: usize,
    pub editing_row: Option<(usize, Vec<EditableCell>)>,
    pub adding_row: Option<Vec<EditableCell>>,
    pub mutation_pending: bool,
}

impl Default for DataTableState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            page_limit: 100,
            current_page: 0,
            editing_row: None,
            adding_row: None,
            mutation_pending: false,
        }
    }
}

pub enum DataTableAction {
    Refresh,
    ChangePage {
        limit: usize,
        offset: usize,
    },
    CopyCell(String),
    ExportCsv,
    SaveRow {
        row_index: usize,
        values: Vec<EditableCell>,
    },
    InsertRow {
        values: Vec<EditableCell>,
    },
    DeleteRow {
        row_index: usize,
    },
}

pub fn render_data_table(
    ui: &mut Ui,
    state: &mut DataTableState,
    result: &QueryResult,
    is_table_view: bool,
    table_name: Option<&str>,
    primary_key_columns: &[String],
    allow_insert: bool,
) -> Option<DataTableAction> {
    let mut action: Option<DataTableAction> = None;

    if result.columns.is_empty() {
        if let Some(affected) = result.rows_affected {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(
                    RichText::new(format!("✔ Command executed successfully"))
                        .color(Color32::from_rgb(100, 220, 100))
                        .strong()
                        .size(16.0),
                );
                ui.label(format!("Rows affected: {}", affected));
                ui.label(
                    RichText::new(format!(
                        "Execution time: {:.2} ms",
                        result.execution_time_ms
                    ))
                    .size(12.0)
                    .color(Color32::GRAY),
                );
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("No data to display")
                        .color(Color32::GRAY)
                        .size(15.0),
                );
                ui.label(
                    RichText::new("Select a table from the sidebar or execute a query.")
                        .color(Color32::DARK_GRAY)
                        .size(12.0),
                );
            });
        }
        return None;
    }

    // Top Toolbar
    ui.horizontal(|ui| {
        if let Some(t_name) = table_name {
            ui.label(RichText::new(format!("📋 {}", t_name)).strong().size(15.0));
            ui.separator();
        }

        if is_table_view && (allow_insert || !primary_key_columns.is_empty()) {
            ui.separator();
            if allow_insert
                && state.adding_row.is_none()
                && state.editing_row.is_none()
                && !state.mutation_pending
                && ui.button("＋ New Row").clicked()
            {
                state.adding_row = Some(
                    result
                        .columns
                        .iter()
                        .map(|_| EditableCell {
                            text: String::new(),
                            is_null: false,
                            use_default: true,
                        })
                        .collect(),
                );
            }
            if state.mutation_pending {
                ui.label(RichText::new("Saving…").color(Color32::YELLOW));
            }
        }

        // Stats
        let row_count = result.rows.len();
        let col_count = result.columns.len();
        ui.label(
            RichText::new(format!("{} rows × {} cols", row_count, col_count))
                .size(12.0)
                .color(Color32::LIGHT_GRAY),
        );
        ui.label(
            RichText::new(format!("⚡ {:.2} ms", result.execution_time_ms))
                .size(12.0)
                .color(Color32::from_rgb(100, 200, 255)),
        );

        ui.separator();

        // In-memory row filter
        ui.label("🔍");
        ui.add(
            egui::TextEdit::singleline(&mut state.search_query)
                .hint_text("Filter rows...")
                .desired_width(160.0),
        );
        if !state.search_query.is_empty() && ui.small_button("✖").clicked() {
            state.search_query.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Export CSV
            if ui
                .button("📥 Export CSV")
                .on_hover_text("Export current rows to CSV file")
                .clicked()
            {
                action = Some(DataTableAction::ExportCsv);
            }

            // Pagination for Table View
            if is_table_view {
                if ui
                    .add_enabled(
                        state.editing_row.is_none()
                            && state.adding_row.is_none()
                            && !state.mutation_pending,
                        egui::Button::new("🔄"),
                    )
                    .on_hover_text("Refresh table data")
                    .clicked()
                {
                    action = Some(DataTableAction::Refresh);
                }

                ui.separator();

                // Next Page
                let has_more = result.rows.len() >= state.page_limit;
                if ui
                    .add_enabled(
                        has_more
                            && state.editing_row.is_none()
                            && state.adding_row.is_none()
                            && !state.mutation_pending,
                        egui::Button::new("▶"),
                    )
                    .on_hover_text("Next page")
                    .clicked()
                {
                    state.current_page += 1;
                    let offset = state.current_page * state.page_limit;
                    action = Some(DataTableAction::ChangePage {
                        limit: state.page_limit,
                        offset,
                    });
                }

                // Page indicator
                ui.label(format!("Page {}", state.current_page + 1));

                // Prev Page
                if ui
                    .add_enabled(
                        state.current_page > 0
                            && state.editing_row.is_none()
                            && state.adding_row.is_none()
                            && !state.mutation_pending,
                        egui::Button::new("◀"),
                    )
                    .on_hover_text("Previous page")
                    .clicked()
                {
                    state.current_page = state.current_page.saturating_sub(1);
                    let offset = state.current_page * state.page_limit;
                    action = Some(DataTableAction::ChangePage {
                        limit: state.page_limit,
                        offset,
                    });
                }
                // Limit selector
                egui::ComboBox::from_id_salt("page_limit_select")
                    .width(75.0)
                    .selected_text(format!("{} / p", state.page_limit))
                    .show_ui(ui, |ui| {
                        for &lim in &[50, 100, 250, 500, 1000] {
                            if ui
                                .add_enabled(
                                    state.editing_row.is_none()
                                        && state.adding_row.is_none()
                                        && !state.mutation_pending,
                                    egui::SelectableLabel::new(
                                        state.page_limit == lim,
                                        format!("{}", lim),
                                    ),
                                )
                                .clicked()
                            {
                                state.page_limit = lim;
                                state.current_page = 0;
                                action = Some(DataTableAction::ChangePage {
                                    limit: state.page_limit,
                                    offset: 0,
                                });
                            }
                        }
                    });
            }
        });
    });

    ui.separator();

    // Filter rows based on search query
    let filter_text = state.search_query.trim().to_lowercase();
    let display_rows: Vec<(usize, &Vec<CellValue>)> = result
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            if filter_text.is_empty() {
                true
            } else {
                r.iter()
                    .any(|cell| cell.text.to_lowercase().contains(&filter_text))
            }
        })
        .collect();

    // Virtualized Table rendering
    let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
    let row_height = text_height + 8.0;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(45.0)) // Row number column
        .columns(
            Column::initial(130.0).at_least(60.0).resizable(true),
            result.columns.len(),
        )
        .column(
            if is_table_view && allow_insert && !primary_key_columns.is_empty() {
                Column::auto().at_least(120.0)
            } else {
                Column::auto().at_least(0.0)
            },
        )
        .header(24.0, |mut header| {
            header.col(|ui| {
                ui.strong(RichText::new("#").color(Color32::GRAY));
            });
            for col_name in &result.columns {
                header.col(|ui| {
                    ui.strong(RichText::new(col_name).color(Color32::from_rgb(220, 230, 240)));
                });
            }
            if is_table_view && allow_insert && !primary_key_columns.is_empty() {
                header.col(|ui| {
                    ui.strong("Actions");
                });
            }
        })
        .body(|body| {
            body.rows(row_height, display_rows.len(), |mut row| {
                let idx = row.index();
                if let Some((original_idx, row_data)) = display_rows.get(idx) {
                    let page_offset = if is_table_view {
                        state.current_page * state.page_limit
                    } else {
                        0
                    };
                    let display_num = page_offset + original_idx + 1;

                    // Row number col
                    row.col(|ui| {
                        ui.label(
                            RichText::new(display_num.to_string())
                                .size(11.0)
                                .color(Color32::DARK_GRAY),
                        );
                    });

                    // Cells
                    for (c_idx, cell) in row_data.iter().enumerate() {
                        row.col(|ui| {
                            let editing = state.editing_row.as_ref().map(|(row, _)| *row)
                                == Some(*original_idx);
                            if editing {
                                if let Some((_, draft)) = state.editing_row.as_mut() {
                                    if let Some(value) = draft.get_mut(c_idx) {
                                        ui.horizontal(|ui| {
                                            ui.add_enabled_ui(
                                                !value.is_null && !value.use_default,
                                                |ui| {
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut value.text)
                                                            .desired_width(95.0),
                                                    );
                                                },
                                            );
                                            ui.checkbox(&mut value.is_null, "NULL");
                                        });
                                    }
                                }
                                return;
                            }
                            let label = if cell.is_null {
                                ui.label(
                                    RichText::new("NULL")
                                        .italics()
                                        .color(Color32::from_rgb(140, 100, 100)),
                                )
                            } else if cell.is_number {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(&cell.text)
                                                .monospace()
                                                .color(Color32::from_rgb(120, 220, 160)),
                                        )
                                    },
                                )
                                .inner
                            } else {
                                ui.label(RichText::new(&cell.text).monospace())
                            };

                            let col_title =
                                result.columns.get(c_idx).map(|s| s.as_str()).unwrap_or("");
                            let resp = label.on_hover_ui(|ui| {
                                ui.label(RichText::new(format!("Column: {}", col_title)).strong());
                                ui.separator();
                                ui.label(RichText::new(&cell.text).monospace());
                                ui.label(
                                    RichText::new("💡 Click to copy value")
                                        .size(10.0)
                                        .color(Color32::GRAY),
                                );
                            });

                            if resp.clicked() {
                                action = Some(DataTableAction::CopyCell(cell.text.clone()));
                            }
                        });
                    }
                    if is_table_view && allow_insert && !primary_key_columns.is_empty() {
                        row.col(|ui| {
                            let editing = state.editing_row.as_ref().map(|(row, _)| *row)
                                == Some(*original_idx);
                            if editing {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            !state.mutation_pending,
                                            egui::Button::new("Save"),
                                        )
                                        .clicked()
                                    {
                                        if let Some((row_index, values)) = &state.editing_row {
                                            action = Some(DataTableAction::SaveRow {
                                                row_index: *row_index,
                                                values: values.clone(),
                                            });
                                            state.mutation_pending = true;
                                        }
                                    }
                                    if ui
                                        .add_enabled(
                                            !state.mutation_pending,
                                            egui::Button::new("Revert"),
                                        )
                                        .clicked()
                                    {
                                        state.editing_row = None;
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            !state.mutation_pending
                                                && state.editing_row.is_none()
                                                && state.adding_row.is_none(),
                                            egui::Button::new("Edit"),
                                        )
                                        .clicked()
                                    {
                                        state.editing_row = Some((
                                            *original_idx,
                                            row_data.iter().map(EditableCell::from_cell).collect(),
                                        ));
                                    }
                                    if ui
                                        .add_enabled(
                                            !state.mutation_pending
                                                && state.editing_row.is_none()
                                                && state.adding_row.is_none(),
                                            egui::Button::new("Delete"),
                                        )
                                        .clicked()
                                    {
                                        action = Some(DataTableAction::DeleteRow {
                                            row_index: *original_idx,
                                        });
                                    }
                                });
                            }
                        });
                    }
                }
            });
        });

    let mut cancel_insert = false;
    if let Some(values) = state.adding_row.as_mut() {
        egui::CollapsingHeader::new("New row values")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("new_table_row_grid")
                    .num_columns(3)
                    .show(ui, |ui| {
                        for (idx, col) in result.columns.iter().enumerate() {
                            if let Some(value) = values.get_mut(idx) {
                                ui.label(RichText::new(col).strong());
                                ui.add_enabled_ui(!value.is_null && !value.use_default, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut value.text)
                                            .desired_width(220.0),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut value.is_null, "NULL").changed()
                                        && value.is_null
                                    {
                                        value.use_default = false;
                                    }
                                    if ui.checkbox(&mut value.use_default, "DEFAULT").changed()
                                        && value.use_default
                                    {
                                        value.is_null = false;
                                    }
                                });
                                ui.end_row();
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!state.mutation_pending, egui::Button::new("Insert row"))
                        .clicked()
                    {
                        action = Some(DataTableAction::InsertRow {
                            values: values.clone(),
                        });
                        state.mutation_pending = true;
                    }
                    if ui
                        .add_enabled(!state.mutation_pending, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        cancel_insert = true;
                    }
                });
            });
    }
    if cancel_insert {
        state.adding_row = None;
    }

    action
}

pub fn export_to_csv(result: &QueryResult, default_name: &str) -> Result<String, String> {
    // Open save dialog or pick default
    let path = rfd::FileDialog::new()
        .set_file_name(&format!("{}.csv", default_name))
        .add_filter("CSV File", &["csv"])
        .save_file();

    let save_path = match path {
        Some(p) => p,
        None => return Err("Export cancelled".to_string()),
    };

    let mut csv_content = String::new();

    // Headers
    let header_line = result
        .columns
        .iter()
        .map(|col| escape_csv_field(col))
        .collect::<Vec<_>>()
        .join(",");
    csv_content.push_str(&header_line);
    csv_content.push('\n');

    // Rows
    for row in &result.rows {
        let row_line = row
            .iter()
            .map(|cell| {
                if cell.is_null {
                    String::new()
                } else {
                    escape_csv_field(&cell.text)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        csv_content.push_str(&row_line);
        csv_content.push('\n');
    }

    std::fs::write(&save_path, csv_content).map_err(|e| format!("Failed to write CSV: {}", e))?;

    Ok(save_path.to_string_lossy().to_string())
}

fn escape_csv_field(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}
