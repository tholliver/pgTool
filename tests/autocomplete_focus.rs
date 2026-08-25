//! Headless regression tests for the SQL-editor autocomplete: focus/caret
//! behavior (existing coverage) plus the ranking fix (best match first,
//! not alphabetical-of-source).
//!
//! Guards against three regressions:
//! 1. Touching `ctx.memory_mut` inside `ctx.input_mut` (egui's Context is
//!    one big RwLock — self-deadlock).
//! 2. Targeting a fabricated `Id::new("sql_editor_textedit")` instead of the
//!    editor's real id (`id_salt` is hashed via `make_persistent_id`, so the
//!    fabricated id belongs to a widget that never renders, and egui's
//!    dead-man's switch then drops ALL keyboard focus).
//! 3. Ranking suggestions by source order (alphabetical) instead of by
//!    closeness to the typed prefix — `drivers` sorting behind
//!    `driver_affiliations` purely because `_` < `s` in ASCII.

use egui::text::{CCursor, CCursorRange};
use egui::widgets::text_edit::TextEditOutput;
use egui::{
    Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, TextEdit, TextStyle,
    pos2, vec2,
};

/// Mirrors `state::CandidateKind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CandidateKind {
    Schema,
    Table,
    Column,
}

fn kind_priority(kind: CandidateKind) -> u8 {
    match kind {
        CandidateKind::Table => 0,
        CandidateKind::Column => 1,
        CandidateKind::Schema => 2,
    }
}

/// Mirrors `state::Candidate`.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Candidate {
    name: String,
    kind: CandidateKind,
}

impl Candidate {
    fn table(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: CandidateKind::Table,
        }
    }
    fn column(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: CandidateKind::Column,
        }
    }
    fn schema(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind: CandidateKind::Schema,
        }
    }
}

#[derive(Default)]
struct AppState {
    sql: String,
    /// Fixture pool the mock resolver filters/ranks from — stands in for
    /// `AppState::autocomplete_candidates()`'s live schema/table/column scan.
    candidate_pool: Vec<Candidate>,
    autocomplete_open: bool,
    autocomplete_matches: Vec<Candidate>,
    autocomplete_selected: usize,
    autocomplete_splice: Option<std::ops::Range<usize>>,
    pending_caret: Option<usize>,
    editor_id: Option<Id>,
}

impl AppState {
    /// Mirrors `AppState::autocomplete_insert`.
    fn autocomplete_insert(&mut self, choice: &str) -> Option<usize> {
        let range = self.autocomplete_splice.take()?;
        if range.start > range.end || range.end > self.sql.len() {
            self.autocomplete_matches.clear();
            self.autocomplete_open = false;
            return None;
        }
        self.sql.replace_range(range.clone(), choice);
        self.autocomplete_matches.clear();
        self.autocomplete_open = false;
        let end_byte = range.start + choice.len();
        Some(self.sql[..end_byte].chars().count())
    }

    /// Mirrors `AppState::accept_autocomplete`.
    fn accept_autocomplete(&mut self) -> Option<usize> {
        let choice = self
            .autocomplete_matches
            .get(self.autocomplete_selected)
            .map(|c| c.name.clone())?;
        self.autocomplete_insert(&choice)
    }
}

/// Mirrors `ui::set_editor_cursor`.
fn set_editor_cursor(ctx: &Context, id: Id, char_idx: usize) {
    let ccursor = CCursor::new(char_idx);
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(ccursor)));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// Mirrors `ui::update_autocomplete` + `compute_word`, now including the
/// ranking step: filter by prefix, then sort by (name length, kind
/// priority, alphabetical) so the closest match to what's typed comes
/// first — this is the behavior the ranking fix added.
fn update_autocomplete(ui: &egui::Ui, st: &mut AppState, output: &TextEditOutput) -> bool {
    let focused = output.response.has_focus();
    let pointer_engaged = ui
        .ctx()
        .input(|i| i.pointer.any_down() || i.pointer.any_click());
    let active = focused || (st.autocomplete_open && pointer_engaged);

    let mut matches: Vec<Candidate> = Vec::new();
    let mut splice: Option<std::ops::Range<usize>> = None;

    if let Some(range) = &output.cursor_range
        && active
    {
        let text = &st.sql;
        let char_idx = usize::from(range.primary.index).min(text.chars().count());
        let cursor_byte = text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(text.len());
        let b = text.as_bytes();
        let mut start = cursor_byte;
        while start > 0 {
            let ch = b[start - 1];
            if ch.is_ascii_whitespace() || ch == b',' || ch == b'(' || ch == b')' {
                break;
            }
            start -= 1;
        }
        let prefix = &text[start..cursor_byte];
        if prefix.chars().count() >= 2 {
            let lower = prefix.to_lowercase();
            let mut filtered: Vec<Candidate> = st
                .candidate_pool
                .iter()
                .filter(|c| {
                    c.name.to_lowercase().starts_with(&lower)
                        && !c.name.eq_ignore_ascii_case(prefix)
                })
                .cloned()
                .collect();
            filtered.sort_by(|a, b| {
                a.name
                    .len()
                    .cmp(&b.name.len())
                    .then_with(|| kind_priority(a.kind).cmp(&kind_priority(b.kind)))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            filtered.truncate(8);
            if !filtered.is_empty() {
                splice = Some(start..cursor_byte);
            }
            matches = filtered;
        }
    }

    if !matches.is_empty() {
        st.autocomplete_selected = 0;
    }
    st.autocomplete_matches = matches;
    st.autocomplete_splice = if st.autocomplete_matches.is_empty() {
        None
    } else {
        splice
    };
    st.autocomplete_open = active && !st.autocomplete_matches.is_empty();
    st.autocomplete_open
}

struct FrameOut {
    output: TextEditOutput,
    popup_row_rects: Vec<Rect>,
}

/// One full frame of the app: pre-draw key handling, SQL editor, popup.
fn run_frame(ctx: &Context, st: &mut AppState, input: RawInput) -> FrameOut {
    let popup_row_rects = std::cell::RefCell::new(Vec::<Rect>::new());
    let output_cell = std::cell::RefCell::new(None::<TextEditOutput>);

    let full_output = ctx.run_ui(input, |ui| {
        // ── 1. Pre-draw key handling (mirrors app/mod.rs) ──────────────
        ui.ctx().input_mut(|i| {
            let ac_open = st.autocomplete_open && !st.autocomplete_matches.is_empty();
            if ac_open
                && (i.consume_key(Modifiers::NONE, Key::Tab)
                    || i.consume_key(Modifiers::NONE, Key::Enter))
            {
                st.pending_caret = st.accept_autocomplete();
            }
        });

        // ── 2. The SQL editor (mirrors ui::draw_sql_editor) ────────────
        if let Some(char_idx) = st.pending_caret.take()
            && let Some(editor_id) = st.editor_id
        {
            set_editor_cursor(ui.ctx(), editor_id, char_idx);
        }
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let font_id = TextStyle::Monospace.resolve(ui.style());
            let job = egui::text::LayoutJob::simple(
                text.as_str().to_owned(),
                font_id,
                egui::Color32::WHITE,
                wrap_width,
            );
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let output = egui::TextEdit::multiline(&mut st.sql)
            .id_salt("sql_editor_textedit")
            .font(TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(4)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        st.editor_id = Some(output.response.id);
        *output_cell.borrow_mut() = Some(output);

        // ── 3. Popup (mirrors ui::show_autocomplete_popup) ─────────────
        let stored = output_cell.borrow();
        let output = stored.as_ref().unwrap();
        let _popup_open = update_autocomplete(ui, st, output);
        if st.autocomplete_open {
            let matches = st.autocomplete_matches.clone();
            let mut chosen: Option<String> = None;
            egui::Area::new(Id::new("sql_autocomplete"))
                .fixed_pos(pos2(200.0, 200.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    for (i, cand) in matches.iter().enumerate() {
                        let resp = ui.button(cand.name.as_str()).on_hover_text("");
                        popup_row_rects.borrow_mut().push(resp.rect);
                        // Real impl: hover syncs the keyboard-highlighted
                        // row (this is the §6 hover/keyboard sync fix).
                        if resp.hovered() {
                            st.autocomplete_selected = i;
                        }
                        if resp.clicked() {
                            chosen = Some(cand.name.clone());
                        }
                    }
                });
            if let Some(choice) = chosen {
                if let Some(new_idx) = st.autocomplete_insert(&choice) {
                    set_editor_cursor(ui.ctx(), output.response.id, new_idx);
                }
            }
        }
    });

    full_output.drop_without_applying_deltas();

    FrameOut {
        output: output_cell.into_inner().unwrap(),
        popup_row_rects: popup_row_rects.into_inner(),
    }
}

fn raw_input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..Default::default()
    }
}

fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn click(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn hover_then_click(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerMoved(pos)]
}

fn release(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
        Event::PointerGone,
    ]
}

fn editor_caret_chars(ctx: &Context, id: Id) -> usize {
    TextEdit::load_state(ctx, id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| usize::from(r.primary.index))
        .expect("editor should have a stored cursor")
}

fn type_and_click_in(ctx: &Context, st: &mut AppState) -> FrameOut {
    let out0 = run_frame(ctx, st, raw_input(vec![]));
    let center = out0.output.response.rect.center();
    run_frame(ctx, st, raw_input(click(center)));
    run_frame(ctx, st, raw_input(release(center)))
}

/// Click into the editor, type a prefix, press Tab to accept "select".
/// Focus must stay on the editor with the caret after the inserted word.
#[test]
fn tab_accept_keeps_focus_and_moves_caret() {
    let ctx = Context::default();
    let mut st = AppState {
        candidate_pool: vec![Candidate::table("select")],
        ..Default::default()
    };
    let out0 = type_and_click_in(&ctx, &mut st);
    let editor_id = out0.output.response.id;
    assert!(
        ctx.memory(|m| m.has_focus(editor_id)),
        "clicking the editor should focus it"
    );

    run_frame(&ctx, &mut st, raw_input(vec![Event::Text("s".into())]));
    run_frame(&ctx, &mut st, raw_input(vec![Event::Text("e".into())]));
    assert_eq!(st.sql, "se");
    assert!(st.autocomplete_open, "popup should be open after typing");
    assert_eq!(st.autocomplete_matches, vec![Candidate::table("select")]);

    run_frame(&ctx, &mut st, raw_input(vec![key(Key::Tab)]));
    assert_eq!(st.sql, "select", "suggestion should be spliced in");
    assert!(
        !st.autocomplete_open,
        "popup must not re-open on the accept frame from a stale caret"
    );
    run_frame(&ctx, &mut st, raw_input(vec![]));

    assert!(
        !st.autocomplete_open,
        "popup should stay closed after accepting"
    );
    assert!(
        ctx.memory(|m| m.has_focus(editor_id)),
        "editor must keep keyboard focus after Tab-accept"
    );
    assert_eq!(
        editor_caret_chars(&ctx, editor_id),
        "select".len(),
        "caret must sit right after the inserted word"
    );

    run_frame(&ctx, &mut st, raw_input(vec![Event::Text("_x".into())]));
    assert_eq!(
        st.sql, "select_x",
        "typing should continue without re-clicking"
    );
}

/// Same scenario, but accepting by clicking a popup row.
#[test]
fn click_accept_keeps_focus_and_moves_caret() {
    let ctx = Context::default();
    let mut st = AppState {
        candidate_pool: vec![Candidate::table("select")],
        ..Default::default()
    };
    let out0 = type_and_click_in(&ctx, &mut st);
    let editor_id = out0.output.response.id;

    run_frame(&ctx, &mut st, raw_input(vec![Event::Text("s".into())]));
    let frame_out = run_frame(&ctx, &mut st, raw_input(vec![Event::Text("e".into())]));
    assert!(st.autocomplete_open);
    let row_rect = *frame_out
        .popup_row_rects
        .first()
        .expect("popup row should have been rendered");

    run_frame(
        &ctx,
        &mut st,
        raw_input(hover_then_click(row_rect.center())),
    );
    run_frame(&ctx, &mut st, raw_input(click(row_rect.center())));
    run_frame(&ctx, &mut st, raw_input(release(row_rect.center())));
    run_frame(&ctx, &mut st, raw_input(vec![]));

    assert_eq!(st.sql, "select", "clicked suggestion should be spliced in");
    assert!(
        ctx.memory(|m| m.has_focus(editor_id)),
        "editor must regain keyboard focus after clicking a suggestion"
    );
    assert_eq!(editor_caret_chars(&ctx, editor_id), "select".len());

    run_frame(&ctx, &mut st, raw_input(vec![Event::Text("!".into())]));
    assert_eq!(st.sql, "select!");
}

/// The bug report: typing `driver` should rank the exact-shape table
/// `drivers` first, not behind `driver_affiliations` / `driver_charges`
/// just because `_` sorts before letters in the alphabetical source list.
#[test]
fn ranks_shortest_match_before_alphabetically_earlier_ones() {
    let ctx = Context::default();
    let mut st = AppState {
        candidate_pool: vec![
            // Deliberately inserted in the "wrong" (alphabetical) order,
            // same shape as `autocomplete_candidates()`'s sorted source.
            Candidate::table("driver_affiliations"),
            Candidate::table("driver_charges"),
            Candidate::table("drivers"),
        ],
        ..Default::default()
    };
    let out0 = type_and_click_in(&ctx, &mut st);
    let _ = out0;

    for ch in "driver".chars() {
        run_frame(&ctx, &mut st, raw_input(vec![Event::Text(ch.to_string())]));
    }
    assert_eq!(st.sql, "driver");
    assert!(st.autocomplete_open);

    let names: Vec<&str> = st
        .autocomplete_matches
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["drivers", "driver_charges", "driver_affiliations"],
        "shortest (closest) match must lead; alphabetical order alone is not enough"
    );
}

/// Tie-break: same length, different kind — table should outrank column.
#[test]
fn ranks_table_before_column_on_length_tie() {
    let ctx = Context::default();
    let mut st = AppState {
        candidate_pool: vec![
            Candidate::column("events"), // len 6, column
            Candidate::table("events2"), // not a real tie, just padding
            Candidate::table("evntab"),  // len 6, table — same length as "events"
        ],
        ..Default::default()
    };
    let out0 = type_and_click_in(&ctx, &mut st);
    let _ = out0;

    for ch in "ev".chars() {
        run_frame(&ctx, &mut st, raw_input(vec![Event::Text(ch.to_string())]));
    }
    assert!(st.autocomplete_open);

    let first = &st.autocomplete_matches[0];
    assert_eq!(
        first.kind,
        CandidateKind::Table,
        "table should win a length tie over column"
    );
}

/// Hovering a row must move the keyboard-highlighted selection to match it
/// (the mouse/keyboard sync fix) — not just leave whatever Tab/ArrowDown
/// last set.
#[test]
fn hover_syncs_keyboard_selection() {
    let ctx = Context::default();
    let mut st = AppState {
        candidate_pool: vec![
            Candidate::table("drivers"),
            Candidate::table("driver_charges"),
        ],
        ..Default::default()
    };
    let out0 = type_and_click_in(&ctx, &mut st);
    let _ = out0;

    for ch in "driver".chars() {
        run_frame(&ctx, &mut st, raw_input(vec![Event::Text(ch.to_string())]));
    }
    let frame_out = run_frame(&ctx, &mut st, raw_input(vec![]));
    assert_eq!(
        st.autocomplete_selected, 0,
        "starts on the top-ranked match"
    );

    let second_row = frame_out.popup_row_rects[1];
    run_frame(
        &ctx,
        &mut st,
        raw_input(hover_then_click(second_row.center())),
    );
    run_frame(&ctx, &mut st, raw_input(vec![]));
    assert_eq!(
        st.autocomplete_selected, 1,
        "hovering the second row should move the keyboard highlight to it"
    );
}
