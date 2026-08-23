pub mod pagination;
pub mod search_bar;
pub mod table;

use crate::app::state::AppState;
use crate::ui::theme::Theme;
use pagination::Action;

pub fn show_panel(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    if state.query.result.is_some() {
        // Snapshot the tiny values the widgets read so the drawing pass
        // below never needs &mut AppState (egui closures can't hold it
        // while row data borrowed from `state` is also in scope).
        let page = state.query.current_page;
        let has_next = state.query.has_next_page;
        let busy = state.is_loading();
        let filter = state.query.filter_text.clone();

        // search bar
        search_bar::show(ui, &mut state.query.filter_text, theme);

        let mut action: Option<Action> = None;

        // Bottom-up layout: the pagination row is drawn FIRST, which pins
        // it to the very bottom edge of the panel at any size; the table
        // then fills exactly the space left above it. No hardcoded heights,
        // so nothing can overflow the panel or leave dead space on resize.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            // drawn first -> sits at the bottom
            action = pagination::show(ui, theme, page, has_next, busy);

            // switch back to top-down: ScrollArea/TableBuilder expect it
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                let Some(result) = state.query.result.as_ref() else {
                    return;
                };
                let cols = result.columns.clone();

                // filter rows client-side
                let filtered_rows: Vec<_> = result
                    .rows
                    .iter()
                    .filter(|row| {
                        if filter.is_empty() {
                            return true;
                        }
                        let f = filter.to_lowercase();
                        row.iter().any(|cell| cell.to_lowercase().contains(&f))
                    })
                    .collect();

                table::show(ui, &cols, &filtered_rows, theme);
            });
        });

        // Apply navigation after drawing — all borrows above have ended.
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
