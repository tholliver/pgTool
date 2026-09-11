pub mod edit_dialog;
pub mod pagination;
pub mod search_bar;
pub mod table;

use crate::app::state::AppState;
use crate::ui::theme::Theme;
use pagination::Action;
use table::TableEvent;

/// A held (owned) "open the cell editor" request, applied after the drawing
/// pass so the immutable `result` borrow from `state` has ended by the time
/// the dialog state is mutated.
struct EditRequest {
    row_idx: usize,
    col_idx: usize,
    schema: String,
    table: String,
    column: String,
    column_type: String,
    nullable: bool,
    identity: Vec<(String, String)>,
}

/// Editing is only offered when the result set is a plain projection of the
/// selected table: every result column must map onto that table (joined
/// results are not editable), and a primary key must be present among the
/// result columns so each row can be targeted unambiguously.
fn editable_sheet(
    result: &crate::db::models::QueryResult,
    table: &crate::db::models::TableInfo,
) -> Option<table::EditableSheet> {
    let mut columns: Vec<Option<table::EditableColumn>> = Vec::with_capacity(result.columns.len());
    for (ci, name) in result.columns.iter().enumerate() {
        let col = table.columns.iter().find(|c| &c.name == name);
        columns.push(col.map(|c| table::EditableColumn {
            schema: table.schema.clone(),
            table: table.name.clone(),
            column: c.name.clone(),
            column_type: result.column_types.get(ci).cloned().unwrap_or_default(),
            nullable: c.nullable,
        }));
    }
    if columns.iter().any(Option::is_none) {
        return None;
    }

    let identity_cols: Vec<usize> = result
        .columns
        .iter()
        .enumerate()
        .filter_map(|(ci, name)| {
            table
                .columns
                .iter()
                .any(|c| c.is_pk && &c.name == name)
                .then_some(ci)
        })
        .collect();
    if identity_cols.is_empty() {
        return None;
    }

    Some(table::EditableSheet {
        columns,
        identity_cols,
    })
}

pub fn show_panel(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    if state.query.result.is_some() {
        // Snapshot the tiny values the widgets read so the drawing pass
        // below never needs &mut AppState (egui closures can't hold it
        // while row data borrowed from `state` is also in scope).
        let page = state.query.current_page;
        let has_next = state.query.has_next_page;
        let rows_returned = state
            .query
            .result
            .as_ref()
            .map(|r| r.rows.len())
            .unwrap_or(0);
        let busy = state.is_loading();
        let filter = state.query.filter_text.clone();

        // Transient "update/copied" feedback, auto-expiring after ~2s; shown
        // on the right side of the filter bar below.
        let feedback_expired = state
            .query
            .copy_feedback
            .as_ref()
            .is_some_and(|fb| fb.since.elapsed() > std::time::Duration::from_secs(2));
        if feedback_expired {
            state.query.copy_feedback = None;
        }
        let feedback: Option<String> = state
            .query
            .copy_feedback
            .as_ref()
            .map(|fb| fb.text.clone());

        // search bar ("Copied …" chip rides along on its right edge)
        search_bar::show(
            ui,
            &mut state.query.filter_text,
            theme,
            feedback.as_deref(),
        );

        let mut action: Option<Action> = None;
        let mut edit_requests: Vec<EditRequest> = Vec::new();

        // Bottom-up layout: the pagination row is drawn FIRST, which pins
        // it to the very bottom edge of the panel at any size; the table
        // then fills exactly the space left above it. No hardcoded heights,
        // so nothing can overflow the panel or leave dead space on resize.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            // drawn first -> sits at the bottom
            action = pagination::show(ui, theme, page, has_next, rows_returned, busy);

            // switch back to top-down: ScrollArea/TableBuilder expect it
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                let Some(result) = state.query.result.as_ref() else {
                    return;
                };

                // Editing is enabled only when the current result is a clean
                // single-table select with a usable primary key.
                let sheet = state
                    .selected_table()
                    .and_then(|t| editable_sheet(result, t));

                // filter rows client-side, remembering each visible row's
                // index into the underlying result set (edit events need it)
                let filtered_rows: Vec<_> = result
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| {
                        if filter.is_empty() {
                            return true;
                        }
                        let f = filter.to_lowercase();
                        row.iter().any(|cell| cell.to_lowercase().contains(&f))
                    })
                    .collect();

                // Focus is only surrendered to a cell on explicit click, so
                // the editor can keep it through scrolling/filtering.
                let editor_id = state.query.editor_id;
                let event = table::show(
                    ui,
                    &result.columns,
                    &result.column_types,
                    &filtered_rows,
                    theme,
                    &mut state.query.selected_cell,
                    editor_id,
                    sheet.as_ref(),
                );
                match event {
                    Some(TableEvent::Copy { column, value }) => {
                        let preview: String = if value.chars().count() > 48 {
                            value.chars().take(48).collect::<String>() + "\u{2026}"
                        } else {
                            value.clone()
                        };
                        state.query.copy_feedback = Some(crate::app::state::CopyFeedback {
                            text: format!("Copied \u{201C}{preview}\u{201D} from {column}"),
                            since: std::time::Instant::now(),
                        });
                    }
                    Some(TableEvent::Edit { row_idx, col_idx }) => {
                        if let Some(col) = sheet
                            .as_ref()
                            .and_then(|s| s.columns.get(col_idx))
                            .and_then(Option::clone)
                        {
                            let identity = sheet
                                .as_ref()
                                .iter()
                                .flat_map(|s| s.identity_cols.iter())
                                .filter_map(|ci| {
                                    let name = result.columns.get(*ci)?.clone();
                                    let value = result.rows.get(row_idx)?.get(*ci)?.clone();
                                    Some((name, value))
                                })
                                .collect();
                            edit_requests.push(EditRequest {
                                row_idx,
                                col_idx,
                                schema: col.schema,
                                table: col.table,
                                column: col.column,
                                column_type: col.column_type,
                                nullable: col.nullable,
                                identity,
                            });
                        }
                    }
                    None => {}
                }
            });
        });

        // Apply navigation and any edit request after drawing — all borrows
        // above have ended.
        match action {
            Some(Action::Prev) => state.prev_page(),
            Some(Action::Next) => state.next_page(),
            Some(Action::Limit(limit)) => {
                state.query.limit_value = limit.to_string();
                state.update_sql();
                state.run_query();
            }
            None => {}
        }
        for req in edit_requests {
            state.open_edit_cell(
                req.row_idx,
                req.col_idx,
                &req.schema,
                &req.table,
                &req.column,
                &req.column_type,
                req.nullable,
                req.identity,
            );
        }
    } else {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(
                egui::RichText::new("No results \u{2014} select a table and run query")
                    .color(theme.text_muted),
            );
        });
    }
}
