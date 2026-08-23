use crate::app::state::AppState;

pub mod connection_dialog;
pub mod results;
pub mod sidebar;
pub mod theme;

pub use theme::{Theme, DARK_ZINC_PURPLE};

pub fn draw(ctx: &egui::Context, state: &mut AppState, theme: &Theme) {
    // connection dialog (modal, drawn on top of everything)
    connection_dialog::show(ctx, &mut state.connection.dialog, theme);

    // left sidebar
    egui::SidePanel::left("sidebar")
        .resizable(true)
        .default_width(220.0)
        .frame(
            egui::Frame::none()
                .fill(theme.panel)
                .inner_margin(egui::Margin::symmetric(4.0, 4.0))
                .stroke(egui::Stroke::new(1.0, theme.border)),
        )
        .show(ctx, |ui| {
            sidebar::show_tree(ui, state, theme);
        });

    // central panel — shared editor/results split when connected,
    // empty state otherwise
    if state.is_connected() {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0)),
            )
            .show(ctx, |ui| {
                let total_height = ui.available_height();
                let handle_height = 6.0;

                const MIN_EDITOR_H: f32 = 140.0; // heading + separator + a few SQL lines
                const MIN_RESULTS_H: f32 = 140.0; // filter bar + pagination + some rows

                // Exact budget: editor + handle + results must always equal
                // the available height, otherwise one half overflows past
                // the window bottom instead of the divider moving.
                let usable = (total_height - handle_height).max(0.0);
                let editor_height = if usable >= MIN_EDITOR_H + MIN_RESULTS_H {
                    let ratio = state.query.results_split_ratio.clamp(0.15, 0.85);
                    (usable * ratio).clamp(MIN_EDITOR_H, usable - MIN_RESULTS_H)
                } else {
                    usable * 0.5 // window too small for minimums — split evenly
                };
                let results_height = usable - editor_height;

                // -- SQL editor, top half --
                ui.allocate_ui(
                    egui::vec2(ui.available_width(), editor_height),
                    |ui| draw_sql_editor(ui, state, theme),
                );

                // -- draggable splitter handle --
                let (handle_rect, handle_resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), handle_height),
                    egui::Sense::drag(),
                );
                let handle_color = if handle_resp.hovered() || handle_resp.dragged() {
                    theme.accent
                } else {
                    theme.border
                };
                ui.painter().rect_filled(handle_rect, 2.0, handle_color);
                if handle_resp.hovered() || handle_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if handle_resp.dragged() {
                    let delta_ratio = handle_resp.drag_delta().y / total_height.max(1.0);
                    state.query.results_split_ratio =
                        (state.query.results_split_ratio + delta_ratio).clamp(0.15, 0.85);
                }

                // -- results, bottom half --
                ui.allocate_ui(egui::vec2(ui.available_width(), results_height), |ui| {
                    egui::Frame::none()
                        .fill(theme.panel)
                        .inner_margin(egui::Margin::symmetric(4.0, 4.0))
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .show(ui, |ui| {
                            results::show_panel(ui, state, theme);
                        });
                });
            });
    } else {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label(
                        egui::RichText::new("Select a connection from the sidebar, or click + to add one")
                            .color(theme.text_muted)
                            .size(14.0),
                    );
                });
            });
    }
}

fn draw_sql_editor(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    ui.heading(
        egui::RichText::new("SQL Query")
            .color(theme.text)
            .size(16.0),
    );
    ui.separator();
    ui.add_space(4.0);

    const CTRL_ROW_H: f32 = 28.0; // LIMIT/OFFSET inputs + Run button row
    // The text area takes ALL height left above the control row, and the
    // ScrollArea keeps its content inside that box — so the editor can never
    // outgrow its half of the split and push the results panel downward.
    let text_area_h = (ui.available_height() - CTRL_ROW_H - 12.0).max(40.0);

    ui.allocate_ui(
        egui::vec2(ui.available_width(), text_area_h),
        |ui| {
            egui::Frame::none()
                .fill(theme.surface)
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("sql_editor_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let editor = egui::TextEdit::multiline(&mut state.query.sql)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .lock_focus(true)
                                .code_editor();
                            ui.add(editor);
                        });
                });
        },
    );

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("LIMIT")
                .color(theme.text_muted)
                .monospace(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.query.limit_value)
                .desired_width(60.0)
                .font(egui::TextStyle::Monospace),
        );
        ui.label(
            egui::RichText::new("OFFSET")
                .color(theme.text_muted)
                .monospace(),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.query.offset_value)
                .desired_width(60.0)
                .font(egui::TextStyle::Monospace),
        );
        ui.separator();
        let can_run = !state.is_loading() && !state.query.sql.trim().is_empty();
        if ui
            .add_enabled(
                can_run,
                theme
                    .accent_button("\u{25B6} Run")
                    .min_size(egui::vec2(80.0, 28.0)),
            )
            .on_hover_text("Run query (Alt+Enter or F5)")
            .clicked()
        {
            state.run_query();
        }
        if let Some(table) = state.selected_table() {
            ui.label(
                egui::RichText::new(format!("{}.{}", table.schema, table.name))
                    .color(theme.text_muted)
                    .monospace(),
            );
        }

        // status / errors, tucked at the right edge of the toolbar
        let status = state.query.status.clone();
        if !status.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if state.is_loading() {
                    ui.spinner();
                }
                ui.label(
                    egui::RichText::new(status)
                        .color(theme.text_muted)
                        .size(11.0),
                );
            });
        }
    });
}
