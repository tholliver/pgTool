use crate::ui::theme::Theme;

/// Compact default width for integer columns: ~7 monospace digits at 12px
/// plus cell padding. Bigger values truncate with a tooltip instead of
/// spilling into the neighboring column.
const INT_COL_W: f32 = 72.0;

/// True for the PostgreSQL integer-ish type names (smallint/int/bigint/oid).
fn is_integer_column(type_name: &str) -> bool {
    matches!(type_name, "INT2" | "INT4" | "INT8" | "OID")
}

/// A result column that maps onto a column of the currently selected table,
/// and therefore can be edited.
#[derive(Clone)]
pub struct EditableColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub column_type: String,
    pub nullable: bool,
}

/// Describes whether (and how) the result grid can be edited **right now**.
///
/// Editing is only offered when the result set is a plain projection of the
/// selected table: every result column must map onto that table (joining
/// other tables disables editing), and the table must expose a primary key
/// among the result columns so each row can be targeted unambiguously.
pub struct EditableSheet {
    /// Index into this vec = result column index; `Some` when that column is
    /// an editable column of the selected table.
    pub columns: Vec<Option<EditableColumn>>,
    /// Result column indices that form the table's primary key. The generated
    /// UPDATE suffixes `WHERE pk1 = … AND pk2 = …` with the row's values.
    pub identity_cols: Vec<usize>,
}

impl EditableSheet {
    fn column_eligible(&self, ci: usize) -> bool {
        self.columns.get(ci).is_some_and(Option::is_some) as bool
    }

    /// A row is a safe UPDATE target only when every PK column is present in
    /// the result and non-NULL (a NULL predicate can never match).
    fn row_eligible(&self, data_row: &[String]) -> bool {
        self.identity_cols
            .iter()
            .all(|k| data_row.get(*k).is_some_and(|v| v != "NULL"))
    }
}

/// What happened in the grid this frame.
pub enum TableEvent {
    /// A cell value was written to the clipboard; show feedback.
    Copy { column: String, value: String },
    /// The user asked to edit a cell (`row_idx` is the index into the
    /// original result rows, stable across client-side filtering).
    Edit { row_idx: usize, col_idx: usize },
}

/// Draws the results table.
///
/// `rows` pairs each visible row with its index into the original result set;
/// selection and keyboard navigation work on visible order, while edit events
/// carry the underlying row index. Returns the event for this frame (copy or
/// requested edit), if any.
pub fn show(
    ui: &mut egui::Ui,
    cols: &[String],
    col_types: &[String],
    rows: &[(usize, &Vec<String>)],
    theme: &Theme,
    selected: &mut Option<(usize, usize)>,
    editor_id: Option<egui::Id>,
    edit: Option<&EditableSheet>,
) -> Option<TableEvent> {
    let n_cols = cols.len();
    if n_cols == 0 {
        return None;
    }

    // The parent layout reserves exactly the space above the pagination row,
    // and auto_shrink([false, false]) stretches the scroll area to fill all
    // of it. No height floor: flooring would overflow small panels.
    let scroll_height = ui.available_height();

    let is_int: Vec<bool> = col_types
        .iter()
        .map(|t| is_integer_column(&t.to_uppercase()))
        .collect();
    let mut event: Option<TableEvent> = None;

    egui::ScrollArea::both()
        .id_salt("results_scroll")
        .max_height(scroll_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            use egui_extras::{Column, TableBuilder};

            let mut tb = TableBuilder::new(ui)
                .striped(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

            for ci in 0..n_cols {
                tb = tb.column(if is_int[ci] {
                    Column::initial(INT_COL_W)
                        .range(48.0..=160.0)
                        .resizable(true)
                } else {
                    Column::initial(160.0).range(80.0..=400.0).resizable(true)
                });
            }

            tb.header(24.0, |mut row| {
                for col in cols {
                    row.col(|ui| {
                        ui.label(
                            egui::RichText::new(col)
                                .color(theme.accent)
                                .monospace()
                                .strong()
                                .size(12.0),
                        );
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let di = row.index(); // index into the visible rows
                    let (row_idx, data_row) = rows[di];
                    for ci in 0..n_cols {
                        let cell = &data_row[ci];
                        row.col(|ui| {
                            // Selection highlight covers the whole cell, like
                            // a spreadsheet cursor rather than just the text.
                            if *selected == Some((di, ci)) {
                                let rect = ui.max_rect();
                                ui.painter().rect_filled(rect, 3.0, theme.accent_muted);
                                ui.painter().rect_stroke(
                                    rect,
                                    3.0,
                                    egui::Stroke::new(1.0, theme.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }

                            let label = egui::Label::new(
                                egui::RichText::new(cell)
                                    .color(theme.text)
                                    .monospace()
                                    .size(12.0),
                            )
                            .truncate()
                            .sense(egui::Sense::click());

                            // Integer columns read right-to-left like numeric
                            // columns in pgAdmin.
                            let resp = if is_int[ci] {
                                let mut content = None;
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        content = Some(ui.add(label));
                                    },
                                );
                                content.unwrap()
                            } else {
                                ui.add(label)
                            };

                            if resp.clicked() {
                                *selected = Some((di, ci));
                                // Let the cell own the copy shortcut: hand
                                // focus back from the SQL editor if it holds
                                // it (lock_focus keeps it there otherwise).
                                if let Some(id) = editor_id {
                                    ui.ctx().memory_mut(|m| m.surrender_focus(id));
                                }
                            }
                            if resp.double_clicked() {
                                *selected = Some((di, ci));
                                if !cell.is_empty() {
                                    ui.ctx().copy_text(cell.clone());
                                    event = Some(TableEvent::Copy {
                                        column: cols[ci].clone(),
                                        value: cell.clone(),
                                    });
                                }
                            }

                            if !cell.is_empty() {
                                resp.clone().on_hover_text(cell);
                            }

                            // Right-click menu: copy, or edit when the grid is
                            // a single-table select with a usable PK.
                            resp.context_menu(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "{} Copy cell value",
                                                egui_phosphor::regular::COPY
                                            ))
                                            .color(theme.text),
                                        )
                                        .min_size(egui::vec2(180.0, 24.0)),
                                    )
                                    .clicked()
                                {
                                    ui.ctx().copy_text(cell.clone());
                                    event = Some(TableEvent::Copy {
                                        column: cols[ci].clone(),
                                        value: cell.clone(),
                                    });
                                    ui.close();
                                }
                                ui.separator();

                                let eligible = edit.is_some_and(|e| {
                                    e.column_eligible(ci) && e.row_eligible(data_row)
                                });
                                if eligible {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(format!(
                                                    "{} Edit cell\u{2026}",
                                                    egui_phosphor::regular::PENCIL_SIMPLE
                                                ))
                                                .color(theme.text),
                                            )
                                            .min_size(egui::vec2(180.0, 24.0)),
                                        )
                                        .clicked()
                                    {
                                        event = Some(TableEvent::Edit {
                                            row_idx,
                                            col_idx: ci,
                                        });
                                        ui.close();
                                    }
                                } else {
                                    let reason = match edit {
                                        None => {
                                            "Editing needs a plain single-table select \
                                                 with a primary key in the result."
                                        }
                                        Some(e) if !e.column_eligible(ci) => {
                                            "This column is not a column of the selected \
                                             table (joined results are not editable)."
                                        }
                                        Some(_) => {
                                            "This row has a NULL primary key and cannot be \
                                             targeted safely."
                                        }
                                    };
                                    ui.add_enabled(
                                        false,
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "{} Edit cell\u{2026}",
                                                egui_phosphor::regular::PENCIL_SIMPLE
                                            ))
                                            .color(theme.text_muted),
                                        )
                                        .min_size(egui::vec2(180.0, 24.0)),
                                    )
                                    .on_disabled_hover_text(reason);
                                }
                            });
                        });
                    }
                });
            });
        });

    // Platform copy shortcut (Cmd+C / Ctrl+C) while a cell is selected and
    // the SQL editor does not own the keystroke for its own selection copy.
    let editor_has_focus = editor_id.is_some_and(|id| ui.memory(|m| m.has_focus(id)));
    if event.is_none()
        && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C))
        && !editor_has_focus
    {
        if let Some((r, c)) = *selected
            && let Some(cell) = rows.get(r).and_then(|(_, row)| row.get(c))
        {
            ui.ctx().copy_text(cell.clone());
            event = Some(TableEvent::Copy {
                column: cols[c].clone(),
                value: cell.clone(),
            });
        }
    }

    event
}
