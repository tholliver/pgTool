use crate::ui::theme::Theme;

/// An interaction in the pagination row, reported back to the caller.
/// Applying it needs `&mut AppState`, which the draw pass deliberately
/// never touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Prev,
    Next,
    Limit(&'static str),
}

/// Draws the pagination row. Pure: reads only copied values, returns what
/// the user clicked (if anything).
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    page: u64,
    has_next_page: bool,
    rows_returned: usize,
    busy: bool,
) -> Option<Action> {
    let mut action = None;

    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !busy && page > 0,
                egui::Button::new(
                    egui::RichText::new(egui_phosphor::regular::CARET_LEFT)
                        .color(theme.text)
                        .size(13.0),
                )
                .fill(theme.surface)
                .corner_radius(4),
            )
            .clicked()
        {
            action = Some(Action::Prev);
        }

        ui.label(
            egui::RichText::new(format!("Page {}", page + 1))
                .color(theme.text_muted)
                .monospace()
                .size(12.0),
        );

        if ui
            .add_enabled(
                !busy && has_next_page,
                egui::Button::new(
                    egui::RichText::new(egui_phosphor::regular::CARET_RIGHT)
                        .color(theme.text)
                        .size(13.0),
                )
                .fill(theme.surface)
                .corner_radius(4),
            )
            .clicked()
        {
            action = Some(Action::Next);
        }

        ui.label(
            egui::RichText::new(format!("\u{00B7} {rows_returned} rows"))
                .color(theme.text_muted)
                .size(11.5),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for limit in ["1000", "100", "50"] {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(limit)
                                .color(theme.text_muted)
                                .monospace()
                                .size(11.0),
                        )
                        .fill(theme.surface)
                        .corner_radius(4),
                    )
                    .clicked()
                {
                    action = Some(Action::Limit(limit));
                }
            }
            ui.label(
                egui::RichText::new("rows per page")
                    .color(theme.text_muted)
                    .size(10.5),
            );
        });
    });

    action
}
