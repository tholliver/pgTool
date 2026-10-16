use crate::app::state::{AppState, SchemaNameMode};
use crate::ui::theme::Theme;

/// Modal used by both "New schema" and "Rename schema".
pub fn show(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    let Some(dlg) = state.query.schema_name_dialog.as_mut() else {
        return;
    };
    let (mut confirm, mut cancel) = (false, false);
    let running = dlg.promise.is_some();
    let (title, action) = match dlg.mode {
        SchemaNameMode::Create => ("New schema", "Create"),
        SchemaNameMode::Rename => ("Rename schema", "Rename"),
    };

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            if let Some(db) = &dlg.database {
                ui.label(
                    egui::RichText::new(format!("Database: {db}"))
                        .color(theme.text_muted)
                        .size(12.0),
                );
            }
            if dlg.mode == SchemaNameMode::Rename {
                ui.label(
                    egui::RichText::new(format!("Current name: {}", dlg.old_name))
                        .color(theme.text_muted)
                        .size(12.0),
                );
            }
            ui.add_space(6.0);
            let edit = ui.add_enabled(
                !running,
                egui::TextEdit::singleline(&mut dlg.name)
                    .hint_text("schema_name")
                    .desired_width(f32::INFINITY),
            );
            if !dlg.focused {
                edit.request_focus();
                dlg.focused = true;
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
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
                let can_run = !running && !dlg.name.trim().is_empty();
                if ui.add_enabled(can_run, egui::Button::new(action)).clicked() {
                    confirm = true;
                }
                if running {
                    ui.spinner();
                }
            });
        });

    if cancel {
        state.query.schema_name_dialog = None;
    } else if confirm {
        state.confirm_schema_name();
    }
}
