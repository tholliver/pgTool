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
    busy: bool,
) -> Option<Action> {
    let mut action = None;

    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !busy && page > 0,
                egui::Button::new(egui::RichText::new("\u{25C0} Prev").color(theme.text).size(12.0))
                    .fill(theme.surface)
                    .rounding(4.0),
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
                egui::Button::new(egui::RichText::new("Next \u{25B6}").color(theme.text).size(12.0))
                    .fill(theme.surface)
                    .rounding(4.0),
            )
            .clicked()
        {
            action = Some(Action::Next);
        }

        ui.separator();

        // Page-size buttons — reset to the first page when changed
        for limit in &["50", "100", "1000"] {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(*limit)
                            .color(theme.text_muted)
                            .monospace()
                            .size(11.0),
                    )
                    .fill(theme.surface)
                    .rounding(4.0),
                )
                .clicked()
            {
                action = Some(Action::Limit(limit));
            }
        }
    });

    action
}
