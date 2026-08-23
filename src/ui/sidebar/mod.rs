pub mod connection_list;
pub mod schema_tree;

use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn show_tree(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    ui.horizontal(|ui| {
        ui.heading(
            egui::RichText::new("Database")
                .color(theme.text)
                .size(14.0)
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // right_to_left: added first = rightmost, so refresh lands
            // just left of the "+" icon
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("+")
                            .color(theme.accent)
                            .size(16.0)
                            .strong(),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .rounding(4.0),
                )
                .on_hover_text("Add Database")
                .clicked()
            {
                state.connection.dialog.open_add();
            }

            let refresh_label = if state.browse_all_active() {
                "Refresh databases"
            } else {
                "Refresh schemas"
            };
            if ui
                .add_enabled(
                    state.is_connected(),
                    egui::Button::new(
                        egui::RichText::new("\u{21BB}")
                            .color(theme.text_muted)
                            .size(14.0)
                            .strong(),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .rounding(4.0),
                )
                .on_hover_text(refresh_label)
                .clicked()
            {
                state.start_post_connect_load();
            }
        });
    });
    ui.separator();
    ui.add_space(4.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Render each saved connection as a root node
        let profiles = state.connection.profiles.clone();
        let active_id = state.connection.active_id.clone();

        if profiles.is_empty() {
            ui.label(
                egui::RichText::new("No connections. Click + to add one.")
                    .color(theme.text_muted)
                    .size(12.0),
            );
            return;
        }

        for profile in &profiles {
            let is_active = active_id.as_deref() == Some(&profile.id);
            let is_expanded = state.query.expanded_connections.contains(&profile.id);

            // Connection root node
            let arrow = if is_expanded { "▼" } else { "▶" };
            let dot = if is_active { "● " } else { "" };
            let label_text = format!("{}{}{}", arrow, dot, profile.generated_name());

            let resp = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(&label_text)
                            .color(if is_active { theme.accent } else { theme.text })
                            .monospace()
                            .size(13.0)
                            .strong(),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);

            if resp.clicked() {
                if is_expanded {
                    state.query.expanded_connections.remove(&profile.id);
                } else {
                    state.query.expanded_connections.insert(profile.id.clone());
                    // If clicking an inactive connection, switch to it
                    if !is_active {
                        state.switch_connection(&profile.id);
                    }
                }
            }

            // Context menu on right-click
            resp.context_menu(|ui| {
                if ui.small_button("Connect").clicked() {
                    if !is_active {
                        state.switch_connection(&profile.id);
                    }
                    ui.close_menu();
                }
                if ui.small_button("Edit").clicked() {
                    state
                        .connection
                        .dialog
                        .open_edit(profile, &profile.password);
                    ui.close_menu();
                }
                if ui.small_button("Delete").clicked() {
                    state.connection.profiles.retain(|p| p.id != profile.id);
                    let _ = crate::connections::save_profiles(&state.connection.profiles);
                    if state.connection.active_id.as_deref() == Some(&profile.id) {
                        state.connection.active_id = None;
                        state.connection.active_pool = None;
                        state.clear_query_state();
                    }
                    ui.close_menu();
                }
            });

            // Show databases/schemas/tables if this is the active expanded connection
            if is_active && is_expanded {
                ui.indent(egui::Id::new(("conn", &profile.id)), |ui| {
                    if profile.browse_all_databases() {
                        if state.query.databases.is_empty() && !state.is_loading() {
                            ui.label(
                                egui::RichText::new("No databases loaded")
                                    .color(theme.text_muted)
                                    .size(12.0),
                            );
                        } else {
                            render_databases(ui, state, theme);
                        }
                    } else if state.query.schemas.is_empty() && !state.is_loading() {
                        ui.label(
                            egui::RichText::new("No schemas loaded")
                                .color(theme.text_muted)
                                .size(12.0),
                        );
                    } else {
                        render_schemas(ui, state, theme);
                    }
                });
            }

            ui.add_space(2.0);
        }
    });
}

/// Database level of the tree — only rendered in "browse all" mode.
fn render_databases(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let db_names: Vec<String> = state.query.databases.iter().map(|d| d.name.clone()).collect();
    for db_name in db_names {
        let is_expanded = state.query.expanded_databases.contains(&db_name);
        let arrow = if is_expanded { "▼" } else { "▶" };

        let resp = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!("  {} {}", arrow, db_name))
                        .color(theme.text)
                        .monospace()
                        .size(12.0)
                        .strong(),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if resp.clicked() {
            if is_expanded {
                state.query.expanded_databases.remove(&db_name);
            } else {
                state.query.expanded_databases.insert(db_name.clone());
                // Lazy-connect + load schemas (reuses an open pool if any).
                state.expand_database(&db_name);
            }
        }

        if is_expanded {
            let loading = state
                .query
                .db_schema_promise
                .as_ref()
                .map(|(n, _)| n == &db_name)
                .unwrap_or(false);
            let schemas = state
                .query
                .databases
                .iter()
                .find(|d| d.name == db_name)
                .map(|d| d.schemas.clone())
                .unwrap_or_default();

            ui.indent(egui::Id::new(("database_indent", &db_name)), |ui| {
                if loading {
                    ui.label(
                        egui::RichText::new("Loading schemas...")
                            .color(theme.text_muted)
                            .size(12.0),
                    );
                } else if schemas.is_empty() {
                    ui.label(
                        egui::RichText::new("No schemas")
                            .color(theme.text_muted)
                            .size(12.0),
                    );
                } else {
                    render_schemas_for(ui, state, theme, Some(&db_name), &schemas);
                }
            });
        }

        ui.add_space(1.0);
    }
}

fn render_schemas(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let schemas = state.query.schemas.clone();
    render_schemas_for(ui, state, theme, None, &schemas);
}

/// Renders schemas + tables. `db_name` is `Some` in browse-all mode, which
/// scopes schema expansion and table selection to that database.
fn render_schemas_for(
    ui: &mut egui::Ui,
    state: &mut AppState,
    theme: &Theme,
    db_name: Option<&str>,
    schemas: &[crate::db::SchemaInfo],
) {
    let mut clicked: Option<(usize, usize)> = None;
    let mut double_clicked: Option<(usize, usize)> = None;

    for (si, schema) in schemas.iter().enumerate() {
        let is_expanded = match db_name {
            Some(db) => state.query.expanded_db_schemas.contains(&(db.to_string(), si)),
            None => state.query.expanded_schemas.contains(&si),
        };
        // In browse-all mode only the active database's rows highlight.
        let is_current_db = match db_name {
            Some(db) => state.query.active_database.as_deref() == Some(db),
            None => true,
        };
        let arrow = if is_expanded { "▼" } else { "▶" };

        let resp = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!("  {} {}", arrow, schema.name))
                        .color(theme.text)
                        .monospace()
                        .size(12.0),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if resp.clicked() {
            match db_name {
                Some(db) => {
                    if is_expanded {
                        state.query.expanded_db_schemas.remove(&(db.to_string(), si));
                    } else {
                        state.query.expanded_db_schemas.insert((db.to_string(), si));
                    }
                }
                None => {
                    if is_expanded {
                        state.query.expanded_schemas.remove(&si);
                    } else {
                        state.query.expanded_schemas.insert(si);
                    }
                }
            }
        }

        if is_expanded {
            ui.indent(egui::Id::new(("schema_indent", db_name, si)), |ui| {
                for (ti, table) in schema.tables.iter().enumerate() {
                    let is_selected =
                        is_current_db && state.query.schema_idx == si && state.query.table_idx == ti;

                    let bg = if is_selected {
                        theme.accent_muted
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let frame = egui::Frame::none()
                        .fill(bg)
                        .rounding(4.0)
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0));

                    let table_resp = frame
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&table.name)
                                        .color(if is_selected {
                                            theme.accent
                                        } else {
                                            theme.text_muted
                                        })
                                        .monospace()
                                        .size(12.0),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                        })
                        .inner;

                    // Accent left border on selected
                    if is_selected {
                        let rect = table_resp.rect;
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() - 2.0, rect.top()),
                                egui::pos2(rect.left() - 2.0, rect.bottom()),
                            ],
                            egui::Stroke::new(2.0, theme.accent),
                        );
                    }

                    if table_resp.double_clicked() {
                        double_clicked = Some((si, ti));
                    } else if table_resp.clicked() {
                        clicked = Some((si, ti));
                    }
                }
            });
        }

        ui.add_space(1.0);
    }

    if let Some((si, ti)) = double_clicked {
        if let Some(db) = db_name {
            state.select_table_in_database(db, si, ti);
            state.run_query();
        } else {
            state.select_table(si, ti);
            state.run_query();
        }
    } else if let Some((si, ti)) = clicked {
        if let Some(db) = db_name {
            state.select_table_in_database(db, si, ti);
        } else {
            state.select_table(si, ti);
        }
    }
}
