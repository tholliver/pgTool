use crate::app::state::AppState;
use crate::ui::theme::Theme;

/// Collapsed-by-default failed-query panel: red left accent, one-line
/// headline, expandable scrollable detail, copy-to-clipboard. Renders
/// nothing when there's no error. Call after the Run/Limit/Offset row.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let Some(error) = state.query.last_error.clone() else {
        return;
    };

    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (bar_rect, _) = ui.allocate_exact_size(
                    egui::vec2(3.0, ui.available_height().max(28.0)),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(bar_rect, 0, theme.danger);
                ui.add_space(9.0);

                ui.vertical(|ui| {
                    draw_headline_row(ui, state, theme, &error.headline);
                    if state.query.error_detail_expanded {
                        ui.add_space(4.0);
                        draw_detail(ui, theme, &error.detail);
                    }
                });
            });
        });
}

fn draw_headline_row(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme, headline: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::X_CIRCLE)
                .color(theme.danger)
                .size(14.0),
        );
        ui.label(
            egui::RichText::new("Query failed")
                .color(theme.text)
                .size(12.0)
                .strong(),
        );
        // One line by construction (QueryError::from_anyhow keeps the
        // headline newline-free); truncate visually rather than wrap.
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("\u{2014} {headline}"))
                    .color(theme.text_muted)
                    .size(11.5),
            )
            .truncate(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, icon) = if state.query.error_detail_expanded {
                ("Hide", egui_phosphor::regular::CARET_UP)
            } else {
                ("Details", egui_phosphor::regular::CARET_DOWN)
            };
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{label} {icon}"))
                        .color(theme.accent)
                        .size(11.0),
                )
                .fill(egui::Color32::TRANSPARENT),
            );
            if resp.clicked() {
                state.query.error_detail_expanded = !state.query.error_detail_expanded;
            }
        });
    });
}

fn draw_detail(ui: &mut egui::Ui, theme: &Theme, detail: &str) {
    egui::ScrollArea::vertical()
        .max_height(110.0)
        .id_salt("error_detail_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(detail)
                    .monospace()
                    .size(11.5)
                    .color(theme.danger.gamma_multiply(0.85)),
            );
        });

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let copy = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{} Copy error", egui_phosphor::regular::COPY))
                    .color(theme.text_muted)
                    .size(10.5),
            )
            .fill(egui::Color32::TRANSPARENT),
        );
        if copy.clicked() {
            // egui 0.36 clipboard write (PlatformOutput commands path).
            ui.ctx().copy_text(detail.to_owned());
        }
    });
}
