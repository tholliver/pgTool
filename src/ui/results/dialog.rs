use crate::db::QueryError;
use crate::ui::theme::Theme;

/// Small colored capsule for the column / type / nullability badges.
pub fn chip(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .size(11.0)
                    .color(color),
            );
        });
}

/// Compact inline error for a modal editor: danger headline with an
/// expandable detail block, matching the query error strip's look but
/// self-contained. `detail_expanded` is the dialog's expand/collapse flag.
pub fn error_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    error: &QueryError,
    detail_expanded: &mut bool,
) {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(egui_phosphor::regular::X_CIRCLE)
                        .color(theme.danger)
                        .size(14.0),
                );
                ui.label(
                    egui::RichText::new(&error.headline)
                        .color(theme.text)
                        .size(12.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, icon) = if *detail_expanded {
                        ("Hide", egui_phosphor::regular::CARET_UP)
                    } else {
                        ("Details", egui_phosphor::regular::CARET_DOWN)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("{label} {icon}"))
                                    .color(theme.accent)
                                    .size(11.0),
                            )
                            .fill(egui::Color32::TRANSPARENT),
                        )
                        .clicked()
                    {
                        *detail_expanded = !*detail_expanded;
                    }
                });
            });
            if *detail_expanded {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .id_salt("error_detail_scroll")
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(&error.detail)
                                .monospace()
                                .size(11.5)
                                .color(theme.danger.gamma_multiply(0.85)),
                        );
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "{} Copy error",
                                    egui_phosphor::regular::COPY
                                ))
                                .color(theme.text_muted)
                                .size(10.5),
                            )
                            .fill(egui::Color32::TRANSPARENT),
                        )
                        .clicked()
                    {
                        ui.ctx().copy_text(error.detail.clone());
                    }
                });
            }
        });
}
