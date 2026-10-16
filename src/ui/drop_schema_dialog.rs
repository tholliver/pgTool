use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn show(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    let Some(dlg) = state.query.drop_schema.as_mut() else {
        return;
    };
    let (mut confirm, mut cancel) = (false, false);
    let running = dlg.promise.is_some();

    egui::Window::new("Delete schema")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(380.0);
            ui.label(
                egui::RichText::new(format!("Delete schema \"{}\"?", dlg.schema))
                    .strong()
                    .size(14.0),
            );
            ui.add_space(4.0);
            let scope = if let Some(db) = &dlg.database {
                format!("Database: {db} · {} table(s)", dlg.table_count)
            } else {
                format!("{} table(s)", dlg.table_count)
            };
            ui.label(
                egui::RichText::new(scope)
                    .color(theme.text_muted)
                    .size(12.0),
            );
            ui.add_space(6.0);
            if crate::db::is_temp_schema(&dlg.schema) {
                ui.label(
                    egui::RichText::new(
                        "Postgres scratch schema for temp tables. Safe to drop only if no \
                         session is using temp tables; Postgres recreates it on demand.",
                    )
                    .color(theme.text_muted)
                    .size(11.5),
                );
            }
            ui.add_enabled(
                !running,
                egui::Checkbox::new(
                    &mut dlg.cascade,
                    "CASCADE: also delete every object inside it and anything depending on them",
                ),
            );
            let note = if dlg.cascade {
                "Everything in this schema is permanently deleted. This cannot be undone."
            } else {
                "Fails if the schema still contains objects (RESTRICT)."
            };
            let note_color = if dlg.cascade {
                theme.danger
            } else {
                theme.text_muted
            };
            ui.label(egui::RichText::new(note).color(note_color).size(11.5));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("Type {} to confirm", dlg.schema))
                    .color(theme.text_muted)
                    .size(12.0),
            );
            ui.add_enabled(
                !running,
                egui::TextEdit::singleline(&mut dlg.confirm_text).desired_width(f32::INFINITY),
            );
            if let Some(err) = &dlg.error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(err).color(theme.danger).size(12.0));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!running, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
                let can_run = !running && dlg.confirm_text == dlg.schema;
                let btn = egui::Button::new(
                    egui::RichText::new("Delete schema").color(egui::Color32::WHITE),
                )
                .fill(theme.danger);
                if ui.add_enabled(can_run, btn).clicked() {
                    confirm = true;
                }
                if running {
                    ui.spinner();
                }
            });
        });

    if cancel {
        state.query.drop_schema = None;
    } else if confirm {
        state.confirm_drop_schema();
    }
}
