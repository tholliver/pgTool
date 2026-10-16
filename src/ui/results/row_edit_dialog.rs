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

/// Types rendered as a multi-line text box in the row form. Everything else
/// gets a single line — numbers, timestamps, booleans, arrays etc. stay
/// compact.
fn is_multiline_type(type_name: &str) -> bool {
    let t = type_name.to_uppercase();
    t.contains("TEXT")
        || t.contains("CHAR")
        || t.contains("JSON")
        || t.contains("XML")
        || t == "NAME"
}

/// Renders the modal full-row editor (results grid → right-click → Edit row).
///
/// Shows the row identity (the PK-based WHERE clause the save will generate)
/// read-only, then a scrollable form with every non-PK column. Values start
/// at the row's current contents; a field is only marked dirty when its text
/// no longer matches the original, so Save emits an UPDATE that SETs exactly
/// the changed columns. The in-flight UPDATE is polled here so success
/// auto-closes the dialog and failure keeps it open with the error on screen.
pub fn show(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    if !state.query.row_edit.open {
        return;
    }

    // dim backdrop — Order::Middle (not Foreground): the window below is
    // created after it in the same frame, so it paints on top of the dim.
    egui::Area::new(egui::Id::new("row_edit_dialog_backdrop"))
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
    if state.poll_edit_row() {
        ctx.request_repaint();
    }
    if !state.query.row_edit.open {
        return;
    }

    let title = format!(
        "Edit row \u{2014} {}.{}",
        state.query.row_edit.schema, state.query.row_edit.table
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
            let d = &mut state.query.row_edit;
            ui.set_min_width(440.0);

            // Row identity — the WHERE clause this save will run. Always
            // shown, always read-only.
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

            // Editable form — every non-PK column of the row. Wrapped in a
            // scroll area so wide tables don't blow out the screen.
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .id_salt("row_edit_fields_scroll")
                .show(ui, |ui| {
                    for f in &mut d.fields {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&f.column)
                                    .color(theme.text_muted)
                                    .size(12.0),
                            );
                            chip(ui, &f.column_type, theme.text_muted);
                            if f.dirty {
                                // small dot next to the label marks changes
                                ui.label(
                                    egui::RichText::new("\u{25CF}")
                                        .color(theme.accent)
                                        .size(10.0),
                                );
                            }
                        });

                        if is_multiline_type(&f.column_type) {
                            let resp = ui.add(
                                egui::TextEdit::multiline(&mut f.value)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("NULL"),
                            );
                            // Dirty tracks a difference from the original, so
                            // typing a value back to what it was un-marks it.
                            if resp.changed() {
                                f.dirty = f.value != f.original_value;
                            }
                        } else {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut f.value)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("NULL"),
                            );
                            if resp.changed() {
                                f.dirty = f.value != f.original_value;
                            }
                        }
                        ui.add_space(6.0);
                    }
                });
            ui.add_space(10.0);

            if let Some(err) = d.error.clone() {
                error_strip(ui, theme, &err, &mut d.detail_expanded);
                ui.add_space(8.0);
            }

            ui.horizontal(|ui| {
                let has_dirty = d.fields.iter().any(|f| f.dirty);
                let label = if d.saving { "Saving\u{2026}" } else { "Save" };
                let save = ui.add_enabled(
                    !d.saving && has_dirty,
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
                if !has_dirty {
                    save.on_disabled_hover_text("No fields changed");
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
        state.close_edit_row();
    } else if pending.save {
        state.save_edit_row();
    }
}
