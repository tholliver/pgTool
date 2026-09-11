use crate::ui::theme::Theme;

/// Transient success/notification line shown on the right of the filter bar
/// (e.g. "Copied …from col"). `None` renders nothing.
pub fn show(
    ui: &mut egui::Ui,
    filter_text: &mut String,
    theme: &Theme,
    feedback: Option<&str>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Filter:")
                .color(theme.text_muted)
                .size(12.0),
        );
        let resp = ui.add(
            egui::TextEdit::singleline(filter_text)
                .desired_width(200.0)
                .hint_text("Search rows...")
                .font(egui::TextStyle::Monospace),
        );
        if resp.changed() {
            // filter updates live — no action needed, next frame will re-filter
        }
        if !filter_text.is_empty() {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("×").color(theme.text_muted).size(14.0))
                        .fill(egui::Color32::TRANSPARENT)
                        .corner_radius(4),
                )
                .on_hover_text("Clear filter")
                .clicked()
            {
                filter_text.clear();
            }
        }

        // "Copied …" chip pinned to the right edge of the bar.
        if let Some(text) = feedback {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} {}",
                        egui_phosphor::regular::CHECK_CIRCLE,
                        text
                    ))
                    .color(theme.success)
                    .size(11.5),
                );
            });
        }
    });
    ui.separator();
}