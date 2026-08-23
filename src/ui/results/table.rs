use crate::ui::theme::Theme;

pub fn show(ui: &mut egui::Ui, cols: &[String], rows: &[&Vec<String>], theme: &Theme) {
    let n_cols = cols.len();
    if n_cols == 0 {
        return;
    }

    // The parent layout reserves exactly the space above the pagination row,
    // and auto_shrink([false, false]) stretches the scroll area to fill all
    // of it. No height floor: flooring would overflow small panels.
    let scroll_height = ui.available_height();

    egui::ScrollArea::both()
        .id_salt("results_scroll")
        .max_height(scroll_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            use egui_extras::{Column, TableBuilder};

            let mut tb = TableBuilder::new(ui)
                .striped(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

            for _ in 0..n_cols {
                tb = tb.column(
                    Column::initial(160.0)
                        .range(80.0..=400.0)
                        .resizable(true),
                );
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
                    let data_row = rows[row.index()];
                    for cell in data_row {
                        row.col(|ui| {
                            let label = egui::Label::new(
                                egui::RichText::new(cell)
                                    .color(theme.text)
                                    .monospace()
                                    .size(12.0),
                            )
                            .truncate()
                            .sense(egui::Sense::hover());
                            let resp = ui.add(label);
                            if !cell.is_empty() {
                                resp.on_hover_text(cell);
                            }
                        });
                    }
                });
            });
        });
}
