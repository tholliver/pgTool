use crate::app::state::AppState;
use crate::ui::theme::Theme;

/// Execution-plan panel shown under the editor toolbar after a successful
/// EXPLAIN: header row (analyze chip, elapsed, pretty/raw toggle, close) plus
/// a collapsible monospace body. Renders nothing while there's no result.
/// Matches the error banner's visual style.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let Some(explain) = state.query.explain_result.clone() else {
        return;
    };

    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .stroke(egui::Stroke::new(0.5, theme.accent_muted))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (bar_rect, _) = ui.allocate_exact_size(
                    egui::vec2(3.0, ui.available_height().max(28.0)),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(bar_rect, 0, theme.accent);
                ui.add_space(9.0);

                ui.vertical(|ui| {
                    draw_header_row(ui, state, theme, &explain);
                    if state.query.explain_panel_open {
                        ui.add_space(4.0);
                        draw_body(ui, state, theme, &explain.plan);
                    }
                });
            });
        });
}

fn draw_header_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    theme: &Theme,
    explain: &crate::db::ExplainResult,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                .color(theme.accent)
                .size(14.0),
        );
        ui.label(
            egui::RichText::new("Execution plan")
                .color(theme.text)
                .size(12.0)
                .strong(),
        );
        if explain.analyze {
            ui.label(
                egui::RichText::new(" ANALYZE")
                    .color(theme.accent)
                    .size(11.0)
                    .monospace()
                    .strong(),
            );
        }
        ui.label(
            egui::RichText::new(format!("\u{2014} {elapsed} ms", elapsed = explain.elapsed_ms))
                .color(theme.text_muted)
                .size(11.5),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(if state.query.explain_panel_open {
                    "Click \u{2193} for plan"
                } else {
                    "Click \u{2192} for plan"
                })
                .color(theme.text_muted)
                .size(10.5),
            )
            .truncate(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Close (×) — clears the result, hiding the panel.
            let close = ui.add(
                egui::Button::new(egui::RichText::new("×").color(theme.text_muted).size(13.0))
                    .fill(egui::Color32::TRANSPARENT),
            );
            if close.clicked() {
                state.query.explain_result = None;
            }

            // Pretty / Raw format toggle.
            let pretty = state.query.explain_pretty;
            let raw_sel = ui.selectable_label(
                !pretty,
                egui::RichText::new("Raw").color(theme.text_muted).size(10.5),
            );
            let pretty_sel = ui.selectable_label(
                pretty,
                egui::RichText::new("Pretty").color(theme.text_muted).size(10.5),
            );
            if raw_sel.clicked() && pretty {
                state.query.explain_pretty = false;
            } else if pretty_sel.clicked() && !pretty {
                state.query.explain_pretty = true;
            }

            // Expand / collapse.
            let (label, caret) = if state.query.explain_panel_open {
                ("Hide", egui_phosphor::regular::CARET_UP)
            } else {
                ("Show", egui_phosphor::regular::CARET_DOWN)
            };
            let toggle = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("{label} {caret}"))
                        .color(theme.accent)
                        .size(11.0),
                )
                .fill(egui::Color32::TRANSPARENT),
            );
            if toggle.clicked() {
                state.query.explain_panel_open = !state.query.explain_panel_open;
            }
        });
    });
}

fn draw_body(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme, plan: &str) {
    let text = format_plan(plan, state.query.explain_pretty);

    egui::ScrollArea::vertical()
        .max_height(160.0)
        .id_salt("explain_plan_scroll")
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(text.clone())
                    .monospace()
                    .size(11.0)
                    .color(theme.text),
            );
        });

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let copy = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{} Copy plan", egui_phosphor::regular::COPY))
                    .color(theme.text_muted)
                    .size(10.5),
            )
            .fill(egui::Color32::TRANSPARENT),
        );
        if copy.clicked() {
            ui.ctx().copy_text(text.clone());
        }
    });
}

/// Pretty-print the JSON plan (default); falls back to the raw text when it
/// isn't valid JSON (e.g. a bare error line from a still-reported failure).
fn format_plan(plan: &str, pretty: bool) -> String {
    if pretty
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(plan)
        && let Ok(s) = serde_json::to_string_pretty(&value)
    {
        return s;
    }
    plan.to_owned()
}