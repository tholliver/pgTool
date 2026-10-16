use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn show(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    let open = state.connection.password_prompt.open;
    if !open {
        return;
    }

    // dim backdrop
    egui::Area::new(egui::Id::new("password_prompt_backdrop"))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let screen = ui.input(|i| i.viewport_rect());
            ui.painter().rect_filled(
                screen,
                egui::CornerRadius::ZERO,
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 150),
            );
        });

    // Copy needed fields for display to avoid borrow issues
    let profile_name = state.connection.password_prompt.profile_name.clone();
    let submitting = state.connection.password_prompt.submitting;
    let _remember = state.connection.password_prompt.remember;
    let error = state.connection.password_prompt.error.clone();

    egui::Window::new("Enter Password")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .order(egui::Order::Foreground)
        .frame(
            egui::Frame::new()
                .fill(theme.panel)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(24, 20))
                .stroke(egui::Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(320.0);

            ui.label(
                egui::RichText::new(format!("Password for \"{}\"", profile_name))
                    .color(theme.text)
                    .size(13.0),
            );
            ui.add_space(8.0);

            field_label(ui, theme, "Password");
            // Password field needs mutable access to state
            let pw_resp = ui.add(
                egui::TextEdit::singleline(&mut state.connection.password_prompt.password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            // Auto-focus the password field
            if !submitting && !pw_resp.has_focus() {
                pw_resp.request_focus();
            }
            ui.add_space(4.0);

            ui.checkbox(
                &mut state.connection.password_prompt.remember,
                "Remember in system keyring",
            );
            ui.add_space(4.0);

            if let Some(err) = error {
                ui.label(egui::RichText::new(err.as_str()).color(theme.danger));
                ui.add_space(4.0);
            }

            let password_empty = state.connection.password_prompt.password.is_empty();
            ui.horizontal(|ui| {
                let label = if submitting { "Testing..." } else { "Connect" };
                let btn = ui.add_enabled(
                    !submitting && !password_empty,
                    egui::Button::new(
                        egui::RichText::new(label)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(theme.accent)
                    .corner_radius(6)
                    .min_size(egui::vec2(130.0, 32.0)),
                );
                if btn.clicked() && !submitting {
                    state.submit_password_prompt();
                }

                if ui
                    .add(
                        egui::Button::new("Cancel")
                            .corner_radius(6)
                            .min_size(egui::vec2(80.0, 32.0)),
                    )
                    .clicked()
                {
                    state.cancel_password_prompt();
                }
            });
        });
}

fn field_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).color(theme.text_muted).size(12.0));
}
