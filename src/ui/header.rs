use crate::app::state::AppState;
use crate::ui::theme::Theme;

/// Header stat chips and static (non-functional) tab strip.
pub fn draw(ui: &mut egui::Ui, state: &AppState, theme: &Theme) {
    let has_error = state.query.last_error.is_some();
    let accent = if has_error {
        theme.danger
    } else {
        theme.accent
    };

    draw_stat_chips(ui, state, theme);
    ui.add_space(10.0);
    draw_tab_strip(ui, theme, accent, has_error);
}

fn draw_stat_chips(ui: &mut egui::Ui, state: &AppState, theme: &Theme) {
    ui.horizontal(|ui| {
        if let Some(dur) = state.query.last_run_duration {
            chip(ui, theme, &format!("last run {}ms", dur.as_millis()), false);
        }
        if let Some((total, idle)) = state.pool_stats() {
            chip(
                ui,
                theme,
                &format!("pool {}/{}", total.saturating_sub(idle as u32), total),
                false,
            );
        }
        chip(
            ui,
            theme,
            &format!("{} this session", state.query.session_query_count),
            false,
        );
        if state.query.session_failed_count > 0 {
            chip(
                ui,
                theme,
                &format!("{} failed", state.query.session_failed_count),
                true,
            );
        }
    });
}

fn chip(ui: &mut egui::Ui, theme: &Theme, text: &str, is_danger: bool) {
    let (bg, fg) = if is_danger {
        (theme.danger.gamma_multiply(0.15), theme.danger)
    } else {
        (theme.surface, theme.text_muted)
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).monospace().size(11.0).color(fg));
        });
}

fn draw_tab_strip(ui: &mut egui::Ui, theme: &Theme, accent: egui::Color32, has_error: bool) {
    ui.horizontal(|ui| {
        let tab_resp = egui::Frame::new()
            .fill(theme.surface)
            .stroke(egui::Stroke::new(0.5, theme.border))
            .corner_radius(egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            })
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if has_error {
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 2.5, theme.danger);
                        ui.add_space(3.0);
                    }
                    ui.label(
                        egui::RichText::new("SQL Query")
                            .color(theme.text)
                            .size(11.5),
                    );
                });
            })
            .response;

        // Active-tab top border, matching the gutter color (purple, or red
        // while an error cascades). Painted manually — egui's Frame stroke
        // is uniform on all four sides.
        let rect = tab_resp.rect;
        ui.painter().hline(
            egui::Rangef::new(rect.left() + 4.0, rect.right() - 4.0),
            rect.top(),
            egui::Stroke::new(1.5, accent),
        );

        // Disabled "+" — visual placeholder for future multi-tab support.
        // No click handler: this is intentionally inert.
        ui.add_enabled(
            false,
            egui::Button::new(
                egui::RichText::new(egui_phosphor::regular::PLUS)
                    .size(13.0)
                    .color(theme.text_muted),
            )
            .fill(egui::Color32::TRANSPARENT),
        )
        .on_disabled_hover_text("Multiple query tabs \u{2014} coming soon");
    });
}
