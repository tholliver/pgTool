use crate::app::state::{AppState, Candidate, CandidateKind};

pub mod appicon;
pub mod connection_dialog;
pub mod error_strip;
pub mod header;
pub mod results;
pub mod sidebar;
pub mod sql_highlight;
pub mod theme;

pub use theme::{DARK_ZINC_PURPLE, Theme};

pub fn draw(
    ui: &mut egui::Ui,
    state: &mut AppState,
    theme: &Theme,
    icon: Option<&egui::TextureHandle>,
) {
    // custom always-dark titlebar (replaces the native Windows chrome)
    draw_titlebar(ui, state, theme, icon);

    // connection dialog (modal, drawn on top of everything)
    connection_dialog::show(ui.ctx(), &mut state.connection.dialog, theme);

    // destructive-statement confirmation (modal on top of the dialog too)
    if let Some(sql) = state.query.pending_confirm_sql.clone() {
        egui::Window::new("Confirm destructive query")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(theme.panel)
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(24, 20))
                    .stroke(egui::Stroke::new(1.5, theme.danger)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    egui::RichText::new("This statement modifies or removes data:")
                        .color(theme.text),
                );
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(theme.surface)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&sql)
                                    .monospace()
                                    .color(theme.danger)
                                    .size(12.0),
                            )
                            .wrap(),
                        );
                    });
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            theme
                                .accent_button("Run anyway")
                                .min_size(egui::vec2(90.0, 26.0)),
                        )
                        .clicked()
                    {
                        state.confirm_run();
                    }
                    if ui.button("Cancel").clicked() {
                        state.query.pending_confirm_sql = None;
                    }
                });
            });
    }

    // left sidebar
    egui::Panel::left("sidebar")
        .resizable(true)
        .default_size(220.0)
        .frame(
            egui::Frame::new()
                .fill(theme.panel)
                .inner_margin(egui::Margin::symmetric(4, 4))
                .stroke(egui::Stroke::new(1.0, theme.border)),
        )
        .show(ui, |ui| {
            sidebar::show_tree(ui, state, theme);
        });

    // central panel — shared editor/results split when connected,
    // empty state otherwise
    if state.is_connected() {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(ui, |ui| {
                let total_height = ui.available_height();
                let handle_height = 6.0;

                const MIN_EDITOR_H: f32 = 170.0; // header + a few SQL lines
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
                ui.allocate_ui(egui::vec2(ui.available_width(), editor_height), |ui| {
                    draw_sql_editor(ui, state, theme)
                });

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
                    egui::Frame::new()
                        .fill(theme.panel)
                        .inner_margin(egui::Margin::symmetric(4, 4))
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .show(ui, |ui| {
                            results::show_panel(ui, state, theme);
                        });
                });
            });
    } else {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label(
                        egui::RichText::new(
                            "Select a connection from the sidebar, or click + to add one",
                        )
                        .color(theme.text_muted)
                        .size(14.0),
                    );
                });
            });
    }
}

fn draw_sql_editor(ui: &mut egui::Ui, state: &mut AppState, theme: &Theme) {
    // header: stat chips + tab strip
    header::draw(ui, state, theme);
    ui.add_space(6.0);

    const CTRL_ROW_H: f32 = 28.0; // toolbar icons + Run button row
    // The text area takes ALL height left above the control row, and the
    // ScrollArea keeps its content inside that box — so the editor can never
    // outgrow its half of the split and push the results panel downward.
    let text_area_h = (ui.available_height() - CTRL_ROW_H - 12.0).max(40.0);

    ui.allocate_ui(egui::vec2(ui.available_width(), text_area_h), |ui| {
        egui::Frame::new()
            .fill(theme.surface)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("sql_editor_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Restore caret + focus BEFORE showing the editor
                        // (Tab/Enter accepts are handled pre-draw in
                        // app/mod.rs), so this frame's galley and
                        // autocomplete recompute see the post-splice text.
                        // Must use the editor's REAL id — `id_salt` is
                        // hashed through `make_persistent_id`, so a
                        // hand-built `Id::new(..)` targets a widget that
                        // never renders, and focus dies with it.
                        if let Some(char_idx) = state.query.pending_caret.take()
                            && let Some(editor_id) = state.query.editor_id
                        {
                            set_editor_cursor(ui.ctx(), editor_id, char_idx);
                        }
                        let mut layouter = crate::ui::sql_highlight::highlight_layouter(theme);
                        let output = egui::TextEdit::multiline(&mut state.query.sql)
                            .id_salt("sql_editor_textedit")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .lock_focus(true)
                            .layouter(&mut layouter)
                            .show(ui);
                        state.query.editor_id = Some(output.response.id);
                        update_autocomplete(ui, state, &output);
                        show_autocomplete_popup(ui, state, theme, &output);
                    });
            });
    });

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        toolbar_icon(
            ui,
            theme,
            egui_phosphor::regular::MAGIC_WAND,
            "Format query",
        );
        toolbar_icon(
            ui,
            theme,
            egui_phosphor::regular::FLOPPY_DISK,
            "Save query",
        );
        toolbar_icon(
            ui,
            theme,
            egui_phosphor::regular::CLOCK_COUNTER_CLOCKWISE,
            "Query history",
        );

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        toolbar_icon(
            ui,
            theme,
            egui_phosphor::regular::MAGNIFYING_GLASS,
            "Explain query",
        );
        toolbar_icon(
            ui,
            theme,
            egui_phosphor::regular::EXPORT,
            "Export results",
        );
        toolbar_icon(ui, theme, egui_phosphor::regular::COPY, "Copy results");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_run = !state.is_loading() && !state.query.sql.trim().is_empty();
            if ui
                .add_enabled(
                    can_run,
                    theme
                        .accent_button(&format!("{} Run", egui_phosphor::regular::PLAY))
                        .min_size(egui::vec2(80.0, 28.0)),
                )
                .on_hover_text("Run query (Alt+Enter or F5)")
                .clicked()
            {
                state.run_query();
            }
            if state.is_loading()
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{} Cancel", egui_phosphor::regular::X))
                                .color(theme.danger),
                        )
                        .fill(egui::Color32::TRANSPARENT),
                    )
                    .clicked()
            {
                if let Some(handle) = state.query.query_cancel.take() {
                    handle.abort();
                }
                state.query.query_promise = None;
                state.query.status = "Cancelled".into();
            }
        });
    });

    ui.add_space(6.0);
    error_strip::draw(ui, state, theme);

    // cell-edit dialog (modal on top of everything, like the others)
    results::edit_dialog::show(ui.ctx(), state, theme);
}

/// Custom window titlebar: app icon, drag-to-move, double-click to
/// maximize/restore, and minimize / maximize / close buttons — all painted
/// with theme colors, so the app never flips white when Windows changes its
/// theme.
fn draw_titlebar(
    ui: &mut egui::Ui,
    state: &AppState,
    theme: &Theme,
    icon: Option<&egui::TextureHandle>,
) {
    const BAR_H: f32 = 34.0;
    const BTN_W: f32 = 42.0;

    egui::Panel::top("app_titlebar")
        .frame(egui::Frame::new().fill(theme.panel))
        .exact_size(BAR_H)
        .show(ui, |ui| {
            let full = ui.max_rect();

            // -- app icon on the left edge --
            let icon_side = 20.0_f32;
            if let Some(icon) = icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(full.left() + 10.0 + icon_side / 2.0, full.center().y),
                    egui::vec2(icon_side, icon_side),
                );
                ui.painter().image(
                    icon.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }

            // -- connection breadcrumb, painted so the drag region owns input --
            let mut cursor_x = full.left() + 10.0 + icon_side + 10.0;
            let center_y = full.center().y;
            let font = egui::FontId::proportional(12.0);

            paint_titlebar_text(ui, &mut cursor_x, center_y, &font, "pgTool", theme.text_muted);
            paint_titlebar_text(ui, &mut cursor_x, center_y, &font, "/", theme.border);

            if let Some(profile) = state
                .connection
                .profiles
                .iter()
                .find(|p| Some(&p.id) == state.connection.active_id.as_ref())
            {
                let dot_r = 3.0;
                ui.painter().circle_filled(
                    egui::pos2(cursor_x + dot_r, center_y),
                    dot_r,
                    theme.success,
                );
                cursor_x += dot_r * 2.0 + 6.0;

                paint_titlebar_text(
                    ui,
                    &mut cursor_x,
                    center_y,
                    &font,
                    &profile.generated_name(),
                    theme.text,
                );

                if let Some(db) = state
                    .query
                    .active_database
                    .clone()
                    .or_else(|| state.selected_table().map(|t| t.schema.clone()))
                {
                    paint_titlebar_text(
                        ui,
                        &mut cursor_x,
                        center_y,
                        &font,
                        egui_phosphor::regular::DATABASE,
                        theme.text_muted,
                    );
                    paint_titlebar_text(ui, &mut cursor_x, center_y, &font, &db, theme.accent);
                }
            }

            // -- window control buttons along the right edge --
            let close_r = egui::Rect::from_min_size(
                egui::pos2(full.right() - BTN_W, full.top()),
                egui::vec2(BTN_W, BAR_H),
            );
            let maxi_r = close_r.translate(egui::vec2(-BTN_W, 0.0));
            let mini_r = maxi_r.translate(egui::vec2(-BTN_W, 0.0));

            let stroke = egui::Stroke::new(1.2, egui::Color32::from_rgb(190, 190, 205));

            if title_button(ui, mini_r, false, "Minimize", |p, c| {
                p.line_segment(
                    [c + egui::vec2(-5.0, 0.0), c + egui::vec2(5.0, 0.0)],
                    stroke,
                );
            }) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }

            let maximized = is_maximized(ui.ctx());
            if title_button(
                ui,
                maxi_r,
                false,
                if maximized { "Restore" } else { "Maximize" },
                move |p, c| {
                    if maximized {
                        // restore: two overlapping squares
                        p.rect_stroke(
                            egui::Rect::from_center_size(
                                c + egui::vec2(2.0, -2.0),
                                egui::vec2(7.0, 7.0),
                            ),
                            0,
                            stroke,
                            egui::StrokeKind::Middle,
                        );
                        p.rect_stroke(
                            egui::Rect::from_center_size(
                                c + egui::vec2(-1.5, 1.5),
                                egui::vec2(7.0, 7.0),
                            ),
                            0,
                            stroke,
                            egui::StrokeKind::Middle,
                        );
                    } else {
                        p.rect_stroke(
                            egui::Rect::from_center_size(c, egui::vec2(9.0, 9.0)),
                            0,
                            stroke,
                            egui::StrokeKind::Middle,
                        );
                    }
                },
            ) {
                toggle_maximize(ui.ctx());
            }

            if title_button(ui, close_r, true, "Close", |p, c| {
                p.line_segment(
                    [c + egui::vec2(-4.5, -4.5), c + egui::vec2(4.5, 4.5)],
                    stroke,
                );
                p.line_segment(
                    [c + egui::vec2(4.5, -4.5), c + egui::vec2(-4.5, 4.5)],
                    stroke,
                );
            }) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }

            // -- draggable strip: everything left of the buttons --
            // min..max corners only: left_top() -> left_top() would be a
            // zero-height rect that hit-testing can never enter.
            let drag_rect =
                egui::Rect::from_min_max(full.left_top(), egui::pos2(mini_r.left(), full.bottom()));
            let drag = ui.interact(
                drag_rect,
                egui::Id::new("titlebar_drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                toggle_maximize(ui.ctx());
            }
        });
}

/// Dummy toolbar icon: visual only, no click handler yet.
fn toolbar_icon(ui: &mut egui::Ui, theme: &Theme, glyph: &str, tooltip: &str) {
    ui.add_enabled(
        false,
        egui::Button::new(egui::RichText::new(glyph).color(theme.text_muted).size(15.0))
            .fill(egui::Color32::TRANSPARENT),
    )
    .on_disabled_hover_text(tooltip);
}

fn paint_titlebar_text(
    ui: &egui::Ui,
    cursor_x: &mut f32,
    center_y: f32,
    font: &egui::FontId,
    text: &str,
    color: egui::Color32,
) {
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), color));
    let pos = egui::pos2(*cursor_x, center_y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley.clone(), color);
    *cursor_x += galley.size().x + 6.0;
}

fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

fn toggle_maximize(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized(ctx)));
}

/// One flat titlebar button; paints its glyph with primitives on top.
fn title_button<F: FnOnce(&egui::Painter, egui::Pos2)>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    danger_hover: bool,
    tooltip: &'static str,
    glyph: F,
) -> bool {
    let hovered = ui.rect_contains_pointer(rect);
    let fill = if hovered {
        if danger_hover {
            egui::Color32::from_rgb(0xC4, 0x2B, 0x3A) // classic red for Close
        } else {
            egui::Color32::from_white_alpha(22)
        }
    } else {
        egui::Color32::TRANSPARENT
    };
    let resp = ui.put(rect, egui::Button::new("").fill(fill).min_size(rect.size()));
    glyph(&ui.painter_at(rect), rect.center());
    resp.on_hover_text(tooltip).clicked()
}

/// Recomputes autocomplete suggestions from the word under the caret.
/// Only runs while the editor is focused; closes the popup when it isn't.
use egui::widgets::text_edit::TextEditOutput;

fn update_autocomplete(ui: &egui::Ui, state: &mut AppState, output: &TextEditOutput) {
    let mut matches: Vec<Candidate> = Vec::new();
    let mut splice: Option<std::ops::Range<usize>> = None;

    // Clicking a popup row surrenders editor focus (egui's default
    // SurrenderFocusOn::Clicks); keep the popup alive while the pointer is
    // mid-click on it, or the row never receives the release and the pick
    // is lost.
    let focused = output.response.has_focus();
    let pointer_engaged = ui
        .ctx()
        .input(|i| i.pointer.any_down() || i.pointer.any_click());
    let active = focused || (state.query.autocomplete_open && pointer_engaged);

    if let Some(range) = &output.cursor_range
        && active
    {
        // CCursor.index counts chars, sql is indexed in bytes
        let text = &state.query.sql;
        let char_idx = range.primary.index.0.min(text.chars().count());
        let byte_idx = text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(text.len());

        if let Some((start, prefix)) = compute_word(text, byte_idx)
            && prefix.chars().count() >= 2
        {
            let lower = prefix.to_lowercase();
            let mut filtered: Vec<Candidate> = state
                .autocomplete_candidates()
                .into_iter()
                .filter(|c| {
                    c.name.to_lowercase().starts_with(&lower)
                        && !c.name.eq_ignore_ascii_case(&prefix)
                })
                .collect();

            // Best match first: shortest name = closest to what's typed
            // (fewest extra chars beyond the prefix), then tables before
            // columns/schemas when tied (you're usually completing a table
            // name), then alphabetical.
            filtered.sort_by(|a, b| {
                a.name
                    .len()
                    .cmp(&b.name.len())
                    .then_with(|| kind_priority(a.kind).cmp(&kind_priority(b.kind)))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            filtered.truncate(8);
            matches = filtered;
            splice = Some(start..byte_idx);
        }
    }

    // Reset the highlighted row whenever the suggestion set changes.
    if state.query.autocomplete_matches != matches {
        state.query.autocomplete_selected = 0;
    }
    state.query.autocomplete_matches = matches;
    state.query.autocomplete_splice = if state.query.autocomplete_matches.is_empty() {
        None
    } else {
        splice
    };
    state.query.autocomplete_open = active && !state.query.autocomplete_matches.is_empty();
}

/// Floating suggestion list anchored under the caret. Click a row (or press
/// Tab/Enter — handled in the global key handler) to splice it into the query.
fn show_autocomplete_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    theme: &Theme,
    output: &TextEditOutput,
) {
    if !state.query.autocomplete_open {
        return;
    }
    let Some(cursor_range) = &output.cursor_range else {
        return;
    };

    // Caret rect is relative to the galley; galley_pos puts it on screen.
    let caret_rect = output.galley.pos_from_cursor(cursor_range.primary);
    let popup_pos = output.galley_pos + egui::vec2(caret_rect.left(), caret_rect.bottom());

    let matches = state.query.autocomplete_matches.clone();
    let selected_idx = state.query.autocomplete_selected;
    let mut chosen: Option<Candidate> = None;

    egui::Area::new(egui::Id::new("sql_autocomplete"))
        .fixed_pos(popup_pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme.surface)
                .stroke(egui::Stroke::new(0.5, theme.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::same(2))
                .show(ui, |ui| {
                    for (i, cand) in matches.iter().enumerate() {
                        let selected = i == selected_idx;
                        let (icon, icon_color) = match cand.kind {
                            CandidateKind::Table => (egui_phosphor::regular::TABLE, theme.accent),
                            CandidateKind::Column => {
                                (egui_phosphor::regular::COLUMNS, theme.text_muted)
                            }
                            CandidateKind::Schema => {
                                (egui_phosphor::regular::FOLDER, theme.text_muted)
                            }
                        };
                        let resp = ui
                            .horizontal(|ui| {
                                ui.label(egui::RichText::new(icon).color(icon_color).size(12.0));
                                ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(&cand.name)
                                            .monospace()
                                            .size(12.0)
                                            .color(if selected {
                                                theme.accent
                                            } else {
                                                theme.text
                                            }),
                                    )
                                    .fill(if selected {
                                        theme.accent_muted
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .min_size(egui::vec2(140.0, 18.0)),
                                )
                            })
                            .inner;
                        if resp.clicked() {
                            chosen = Some(cand.clone());
                        }
                        if resp.hovered() {
                            state.query.autocomplete_selected = i;
                        }
                    }
                });
        });

    if let Some(choice) = chosen
        && let Some(new_idx) = state.autocomplete_insert(&choice.name)
    {
        set_editor_cursor(ui.ctx(), output.response.id, new_idx);
    }
}

/// Place the SQL editor's caret at `char_idx` (char-indexed, matching
/// egui's `CCursor`) and hand focus back to the editor. `id` MUST be the
/// editor's real id (e.g. `TextEditOutput::response.id`).
fn set_editor_cursor(ctx: &egui::Context, id: egui::Id, char_idx: usize) {
    let ccursor = egui::text::CCursor::new(char_idx);
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// Ranking weight for equal-length candidates: tables first (the usual
/// completion target), then columns, then schemas.
fn kind_priority(kind: CandidateKind) -> u8 {
    match kind {
        CandidateKind::Table => 0,
        CandidateKind::Column => 1,
        CandidateKind::Schema => 2,
    }
}

/// Word ending at the caret: stops at whitespace, `,` or parens; a leading
/// qualifier (`schema.table`) narrows the match to the last segment. Returns
/// (splice_start_byte, word).
fn compute_word(text: &str, cursor_byte: usize) -> Option<(usize, String)> {
    let b = text.as_bytes();
    let mut start = cursor_byte;
    while start > 0 {
        let ch = b[start - 1];
        if ch.is_ascii_whitespace() || ch == b',' || ch == b'(' || ch == b')' {
            break;
        }
        start -= 1;
    }
    let word = &text[start..cursor_byte];
    if word.is_empty() {
        return None;
    }
    match word.rsplit_once('.') {
        Some((_, seg)) => Some((cursor_byte - seg.len(), seg.to_string())),
        None => Some((start, word.to_string())),
    }
}
