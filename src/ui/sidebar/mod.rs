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
                    .corner_radius(4),
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
                        egui::RichText::new(egui_phosphor::regular::ARROW_CLOCKWISE)
                            .color(theme.text_muted)
                            .size(14.0)
                            .strong(),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .corner_radius(4),
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

    // tree filter — narrows the table lists below by name
    ui.add(
        egui::TextEdit::singleline(&mut state.query.tree_filter)
            .hint_text("Filter tables...")
            .desired_width(f32::INFINITY),
    );
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

            // Connection card: active ones get a filled/stroked surface plus a
            // painted success dot; inactive ones get an explicit play button
            // to become active ("run queries against this one").
            let card_fill = if is_active {
                theme.surface
            } else {
                egui::Color32::TRANSPARENT
            };
            let card_stroke = if is_active {
                theme.accent_muted
            } else {
                theme.border
            };

            let card_resp = egui::Frame::new()
                .fill(card_fill)
                .corner_radius(8)
                .stroke(egui::Stroke::new(0.5, card_stroke))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // left-rail active dot — painted, not text, so it's a
                        // real colored mark independent of font rendering
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                        if is_active {
                            ui.painter()
                                .circle_filled(dot_rect.center(), 3.0, theme.success);
                        }

                        // A saved profile is really a server; the plug glyph
                        // says "connection" while the tree below shows its
                        // databases.
                        ui.add(egui::Label::new(
                            egui::RichText::new(egui_phosphor::regular::PLUGS_CONNECTED)
                                .color(theme.accent)
                                .size(14.0),
                        ));

                        let name_resp = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(profile.generated_name())
                                        .color(if is_active {
                                            theme.text
                                        } else {
                                            theme.text_muted
                                        })
                                        .size(12.5)
                                        .strong(),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);

                        // right-aligned: play-to-activate (inactive) or
                        // expand caret (active) — never both
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if is_active {
                                let caret = if is_expanded {
                                    egui_phosphor::regular::CARET_DOWN
                                } else {
                                    egui_phosphor::regular::CARET_RIGHT
                                };
                                let caret_resp = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(caret)
                                            .color(theme.text_muted)
                                            .size(12.0),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .corner_radius(4),
                                );
                                if caret_resp.clicked() {
                                    if is_expanded {
                                        state.query.expanded_connections.remove(&profile.id);
                                    } else {
                                        state.query.expanded_connections.insert(profile.id.clone());
                                    }
                                }
                            } else {
                                let play = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(egui_phosphor::regular::PLAY)
                                                .color(theme.success)
                                                .size(13.0),
                                        )
                                        .fill(egui::Color32::TRANSPARENT)
                                        .corner_radius(4),
                                    )
                                    .on_hover_text("Connect");
                                if play.clicked() {
                                    state.switch_connection(&profile.id);
                                    state.query.expanded_connections.insert(profile.id.clone());
                                }
                            }
                        });

                        name_resp
                    })
                    .inner
                });

            let name_resp = card_resp.inner;
            if name_resp.clicked() {
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
            name_resp.context_menu(|ui| {
                if ui.small_button("Connect").clicked() {
                    if !is_active {
                        state.switch_connection(&profile.id);
                    }
                    ui.close();
                }
                if ui.small_button("Edit").clicked() {
                    state
                        .connection
                        .dialog
                        .open_edit(profile, &profile.password);
                    ui.close();
                }
                if ui.small_button("Delete").clicked() {
                    state.connection.profiles.retain(|p| p.id != profile.id);
                    let _ = crate::connections::save_profiles(&state.connection.profiles);
                    if state.connection.active_id.as_deref() == Some(&profile.id) {
                        state.connection.active_id = None;
                        state.connection.active_pool = None;
                        state.clear_query_state();
                    }
                    ui.close();
                }
            });

            // Show databases/schemas/tables if this is the active expanded connection
            if is_active && is_expanded {
                ui.indent(egui::Id::new(("conn", &profile.id)), |ui| {
                    // Rounded left padding keeps the tree visually nested
                    // under the card above it.
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 14,
                            ..Default::default()
                        })
                        .show(ui, |ui| {
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
                });
            }

            ui.add_space(2.0);
        }
    });
}

/// Database level of the tree — only rendered in "browse all" mode.
fn render_databases(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    let db_names: Vec<String> = state
        .query
        .databases
        .iter()
        .map(|d| d.name.clone())
        .collect();
    for db_name in db_names {
        let is_expanded = state.query.expanded_databases.contains(&db_name);
        let arrow = if is_expanded {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };

        let resp = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "  {} {} {}",
                        arrow,
                        egui_phosphor::regular::DATABASE,
                        db_name
                    ))
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
            let (schemas, loaded) = state
                .query
                .databases
                .iter()
                .find(|d| d.name == db_name)
                .map(|d| (d.schemas.clone(), d.loaded))
                .unwrap_or_default();

            if !loading && !loaded {
                state.expand_database(&db_name);
            }

            ui.indent(egui::Id::new(("database_indent", &db_name)), |ui| {
                if loading {
                    ui.label(
                        egui::RichText::new("Loading schemas...")
                            .color(theme.text_muted)
                            .size(12.0),
                    );
                } else if schemas.is_empty() {
                    ui.label(
                        egui::RichText::new(if loaded {
                            "No schemas"
                        } else {
                            "Loading schemas..."
                        })
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
///
/// Table rows are painted manually (allocate -> interact -> paint) so the
/// hover fill can react to the pointer, unlike a Frame-wrapped Label whose
/// background must be decided before the frame is drawn.
fn render_schemas_for(
    ui: &mut egui::Ui,
    state: &mut AppState,
    theme: &Theme,
    db_name: Option<&str>,
    schemas: &[crate::db::SchemaInfo],
) {
    const ROW_H: f32 = 20.0;

    let filter = state.query.tree_filter.trim().to_lowercase();
    let filter_active = !filter.is_empty();

    let mut clicked: Option<(usize, usize)> = None;
    let mut double_clicked: Option<(usize, usize)> = None;

    for (si, schema) in schemas.iter().enumerate() {
        let is_expanded = match db_name {
            Some(db) => state
                .query
                .expanded_db_schemas
                .contains(&(db.to_string(), si)),
            None => state.query.expanded_schemas.contains(&si),
        };
        // In browse-all mode only the active database's rows highlight.
        let is_current_db = match db_name {
            Some(db) => state.query.active_database.as_deref() == Some(db),
            None => true,
        };
        // An active filter forces tables visible regardless of expansion.
        let show_tables = is_expanded || filter_active;
        let arrow = if show_tables {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };

        let resp = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "  {} {} {}",
                        arrow,
                        egui_phosphor::regular::FOLDER,
                        schema.name
                    ))
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
                        state
                            .query
                            .expanded_db_schemas
                            .remove(&(db.to_string(), si));
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

        if show_tables {
            // Preserve original indices — selection uses (schema_idx, table_idx).
            let matching: Vec<(usize, &crate::db::TableInfo)> = schema
                .tables
                .iter()
                .enumerate()
                .filter(|(_, t)| t.name.to_lowercase().contains(&filter))
                .collect();

            ui.indent(egui::Id::new(("schema_indent", db_name, si)), |ui| {
                if matching.is_empty() {
                    if filter_active && !schema.tables.is_empty() {
                        ui.label(
                            egui::RichText::new("No matching tables")
                                .color(theme.text_muted)
                                .size(12.0),
                        );
                    }
                } else {
                    for (ti, table) in matching {
                        let is_selected = is_current_db
                            && state.query.schema_idx == si
                            && state.query.table_idx == ti;

                        // 1. reserve the row's space, 2. ask for interaction,
                        // 3. NOW we know hover/selection and can paint the bg.
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_H),
                            egui::Sense::hover(),
                        );
                        let resp = ui.interact(
                            rect.expand2(egui::vec2(2.0, 0.0)),
                            egui::Id::new(("table_row", db_name, si, ti)),
                            egui::Sense::click(),
                        );

                        let bg = if is_selected {
                            theme.accent_muted
                        } else if resp.hovered() {
                            theme.surface
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        if bg != egui::Color32::TRANSPARENT {
                            ui.painter().rect_filled(rect, 4, bg);
                        }

                        let text_color = if is_selected {
                            theme.accent
                        } else if resp.hovered() {
                            theme.text
                        } else {
                            theme.text_muted
                        };

                        let font_id = egui::FontId::monospace(12.0);
                        let painter = ui.painter_at(rect);
                        let left = rect.left() + 6.0;
                        let mid_y = rect.center().y;

                        painter.text(
                            egui::pos2(left + 7.0, mid_y),
                            egui::Align2::CENTER_CENTER,
                            egui_phosphor::regular::TABLE,
                            font_id.clone(),
                            text_color,
                        );

                        let name_galley =
                            painter.layout_no_wrap(table.name.clone(), font_id.clone(), text_color);
                        let count_max_width = if table.row_count.is_some() {
                            painter
                                .layout_no_wrap(
                                    "00000000".to_string(),
                                    font_id.clone(),
                                    theme.text_muted,
                                )
                                .size()
                                .x
                        } else {
                            0.0
                        };
                        let fits_with_count =
                            name_galley.size().x <= rect.width() - 24.0 - count_max_width - 12.0;
                        painter.galley(
                            egui::pos2(left + 18.0, mid_y - name_galley.size().y / 2.0),
                            name_galley,
                            text_color,
                        );

                        if let Some(count) = table.row_count
                            && fits_with_count
                        {
                            let galley = painter.layout_no_wrap(
                                count.to_string(),
                                font_id,
                                theme.text_muted,
                            );
                            let w = galley.size().x;
                            painter.galley(
                                egui::pos2(rect.right() - 8.0 - w, mid_y - galley.size().y / 2.0),
                                galley,
                                theme.text_muted,
                            );
                        }

                        // Accent left border on selected
                        if is_selected {
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left() - 2.0, rect.top()),
                                    egui::pos2(rect.left() - 2.0, rect.bottom()),
                                ],
                                egui::Stroke::new(2.0, theme.accent),
                            );
                        }

                        if resp.double_clicked() {
                            double_clicked = Some((si, ti));
                        } else if resp.clicked() {
                            clicked = Some((si, ti));
                        }
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
