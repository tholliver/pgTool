use crate::app::state::AppState;
use crate::ui::results::dialog::{chip, error_strip};
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
        .order(egui::Order::Middle)
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
        .order(egui::Order::Foreground)
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
                error_strip(ui, theme, &err, &mut d.detail_expanded);
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
