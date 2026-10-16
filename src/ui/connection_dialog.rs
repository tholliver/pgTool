use crate::app::state::ConnectionDialog;
use crate::connections::SslMode;
use crate::ui::theme::Theme;

pub fn show(ctx: &egui::Context, dialog: &mut ConnectionDialog, theme: &Theme) {
    if !dialog.open {
        return;
    }

    // dim backdrop
    egui::Area::new(egui::Id::new("dialog_backdrop"))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let screen = ui.input(|i| i.viewport_rect());
            ui.painter().rect_filled(
                screen,
                egui::CornerRadius::ZERO,
                egui::Color32::from_black_alpha(110),
            );
            // Consume pointer events so nothing behind the dialog reacts.
            ui.allocate_rect(screen, egui::Sense::click_and_drag());
        });

    let title = if dialog.editing_id.is_some() {
        "Edit Connection"
    } else {
        "Add Connection"
    };

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .order(egui::Order::Foreground)
        .frame(
            egui::Frame::new()
                .fill(theme.surface)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(24, 20))
                .stroke(egui::Stroke::new(1.0, theme.border))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(120),
                }),
        )
        .show(ctx, |ui| {
            ui.set_min_width(320.0);

            field_label(ui, theme, "Name");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.name)
                    .desired_width(f32::INFINITY)
                    .hint_text("e.g. Local Dev"),
            );
            ui.add_space(4.0);

            field_label(ui, theme, "Host");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.host)
                    .desired_width(f32::INFINITY)
                    .hint_text("localhost"),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    field_label(ui, theme, "Port");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.port)
                            .desired_width(80.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                ui.vertical(|ui| {
                    field_label(ui, theme, "Database (leave empty to show all)");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.database)
                            .desired_width(f32::INFINITY)
                            .hint_text("all databases"),
                    );
                });
            });
            ui.add_space(4.0);

            field_label(ui, theme, "Username");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.username)
                    .desired_width(f32::INFINITY)
                    .hint_text("postgres"),
            );
            ui.add_space(4.0);

            field_label(ui, theme, "Password");
            ui.add(
                egui::TextEdit::singleline(&mut dialog.password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);

            field_label(ui, theme, "SSL Mode");
            egui::ComboBox::from_id_salt("ssl_mode")
                .selected_text(dialog.ssl_mode.as_str())
                .show_ui(ui, |ui| {
                    for mode in SslMode::all() {
                        ui.selectable_value(&mut dialog.ssl_mode, mode.clone(), mode.as_str());
                    }
                });
            ui.add_space(8.0);

            if let Some(err) = &dialog.error {
                ui.label(egui::RichText::new(err.as_str()).color(theme.danger));
                ui.add_space(4.0);
            }

            // Duplicate warning with "Save anyway" button
            if let Some((warn, _, _)) = &dialog.pending_duplicate {
                ui.label(egui::RichText::new(warn.as_str()).color(theme.accent));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Save anyway")
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(theme.accent)
                            .corner_radius(6)
                            .min_size(egui::vec2(130.0, 32.0)),
                        )
                        .clicked()
                    {
                        // Signal to AppState to confirm duplicate save
                        dialog.confirm_duplicate_save_requested = true;
                    }
                    if ui
                        .add(
                            egui::Button::new("Cancel")
                                .corner_radius(6)
                                .min_size(egui::vec2(80.0, 32.0)),
                        )
                        .clicked()
                    {
                        dialog.pending_duplicate = None;
                        dialog.testing = false;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    let label = if dialog.testing {
                        "Testing..."
                    } else {
                        "Test & Connect"
                    };
                    let btn = ui.add_enabled(
                        !dialog.testing,
                        egui::Button::new(
                            egui::RichText::new(label)
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(theme.accent)
                        .corner_radius(6)
                        .min_size(egui::vec2(130.0, 32.0)),
                    );
                    if btn.clicked() && !dialog.testing {
                        dialog.start_test();
                    }

                    if ui
                        .add(
                            egui::Button::new("Cancel")
                                .corner_radius(6)
                                .min_size(egui::vec2(80.0, 32.0)),
                        )
                        .clicked()
                    {
                        dialog.close();
                    }
                });
            }
        });
}

fn field_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).color(theme.text_muted).size(12.0));
}
