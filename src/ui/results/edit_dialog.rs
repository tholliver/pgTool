use crate::app::state::AppState;
use crate::ui::theme::Theme;

/// Keys held by the dialog while the show() closure and the surrounding
/// &mut borrow of `state` are live; the actions are applied once the window
/// borrow has ended, mirroring the results panel's action-queue pattern.
#[derive(Default)]
struct Pending {
    save: bool,
    cancel: bool,
}

/// Renders the modal cell editor (results grid → right-click → Edit cell).
///
/// Shows the row identity (the PK-based WHERE clause the save will generate),
/// a monospace value box seeded with the current cell, and an inline,
/// expandable error strip on failure. The in-flight UPDATE is polled here so
/// success auto-closes the dialog and failure keeps it open with the error on
/// screen.
pub fn show(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    if !state.query.edit.open {
        return;
    }

    // dim backdrop
    egui::Area::new(egui::Id::new("edit_dialog_backdrop"))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let screen = ui.input(|i| i.viewport_rect());
            ui.painter().rect_filled(
                screen,
                egui::CornerRadius::ZERO,
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 150),
            );
        });

    // Apply the outcome of any in-flight save (success closes the dialog and
    // refreshes the grid; failure leaves it open with the error visible).
    if state.poll_edit() {
        ctx.request_repaint();
    }
    if !state.query.edit.open {
        return;
    }

    let title = format!(
        "Edit {}.{}.{}",
        state.query.edit.schema, state.query.edit.table, state.query.edit.column
    );
    let mut pending = Pending::default();

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::new()
                .fill(theme.panel)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(20, 16))
                .stroke(egui::Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            let d = &mut state.query.edit;
            ui.set_min_width(440.0);

            // Column meta chips.
            ui.horizontal_wrapped(|ui| {
                chip(ui, &d.column, theme.accent);
                chip(ui, &d.column_type, theme.text_muted);
                chip(
                    ui,
                    if d.nullable { "NULLABLE" } else { "NOT NULL" },
                    if d.nullable {
                        theme.success
                    } else {
                        theme.danger
                    },
                );
            });
            ui.add_space(10.0);

            // Row identity — the WHERE clause this edit will run.
            ui.label(
                egui::RichText::new("Row identity")
                    .color(theme.text_muted)
                    .size(12.0),
            );
            for (name, value) in &d.identity {
                ui.label(
                    egui::RichText::new(format!("\u{2022} {name} = {value}"))
                        .monospace()
                        .size(11.5)
                        .color(theme.text),
                );
            }
            ui.add_space(10.0);

            ui.label(
                egui::RichText::new("New value")
                    .color(theme.text_muted)
                    .size(12.0),
            );
            ui.add(
                egui::TextEdit::multiline(&mut d.value)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("New cell value"),
            );
            ui.add_space(10.0);

            if let Some(err) = d.error.clone() {
                error_strip(ui, d, theme, &err);
                ui.add_space(8.0);
            }

            ui.horizontal(|ui| {
                let label = if d.saving { "Saving\u{2026}" } else { "Save" };
                let save = ui.add_enabled(
                    !d.saving,
                    egui::Button::new(
                        egui::RichText::new(format!("{} {label}", egui_phosphor::regular::CHECK))
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(theme.accent)
                    .corner_radius(6)
                    .min_size(egui::vec2(110.0, 32.0)),
                );
                if save.clicked() && !d.saving {
                    pending.save = true;
                }

                if ui
                    .add(
                        egui::Button::new("Cancel")
                            .corner_radius(6)
                            .min_size(egui::vec2(80.0, 32.0)),
                    )
                    .clicked()
                {
                    pending.cancel = true;
                }
            });
        });

    if pending.cancel {
        state.close_edit_cell();
    } else if pending.save {
        state.save_edit_cell();
    }
}

/// Small colored capsule for the column / type / nullability badges.
fn chip(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
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

/// Compact inline error for the dialog: danger headline with an expandable
/// detail block, matching the query error strip's look but self-contained.
fn error_strip(
    ui: &mut egui::Ui,
    d: &mut crate::app::state::EditDialog,
    theme: &Theme,
    error: &crate::db::QueryError,
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
                    let (label, icon) = if d.detail_expanded {
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
                        d.detail_expanded = !d.detail_expanded;
                    }
                });
            });
            if d.detail_expanded {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .id_salt("edit_error_detail_scroll")
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
