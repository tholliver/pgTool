use crate::ui::theme::Theme;

pub fn show(ui: &mut egui::Ui, filter_text: &mut String, theme: &Theme) {
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
                    egui::Button::new(
                        egui::RichText::new("×")
                            .color(theme.text_muted)
                            .size(14.0),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .rounding(4.0),
                )
                .on_hover_text("Clear filter")
                .clicked()
            {
                filter_text.clear();
            }
        }
    });
    ui.separator();
}
