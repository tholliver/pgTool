//! Headless regression tests for the error panel's Details toggle and
//! Copy-error button. Mirrors `ui/error_strip.rs`'s rendering shape by
//! hand — this crate has no `[lib]` target (Cargo.toml declares only a
//! `[[bin]]`), so files under `tests/` cannot import the real widget and
//! must re-state the behavior under test. See the same note in
//! `autocomplete_focus.rs`.
//!
//! Locks down two behaviors:
//! 1. The Details/Hide button flips the expanded flag in both directions.
//! 2. Copy error pushes the FULL multi-line detail text onto egui's real
//!    clipboard output path (`OutputCommand::CopyText`), not just the
//!    collapsed headline.

use egui::{Context, Event, OutputCommand, PointerButton, Pos2, RawInput, Rect, vec2};

struct ErrorPanelState {
    /// Mirrors `state.query.error_detail_expanded`.
    expanded: bool,
    detail: String,
}

#[derive(Default)]
struct FrameOut {
    details_button_rect: Option<Rect>,
    copy_button_rect: Option<Rect>,
    copied_text: Option<String>,
}

/// One frame of the error strip (mirrors `error_strip::draw`).
fn draw_frame(ctx: &Context, st: &mut ErrorPanelState, input: RawInput) -> FrameOut {
    let out = std::cell::RefCell::new(FrameOut::default());

    let full_output = ctx.run_ui(input, |ui| {
        // -- headline row: X icon, "Query failed", one-line headline, toggle --
        ui.horizontal(|ui| {
            ui.label("Query failed");
            let label = if st.expanded { "Hide" } else { "Details" };
            let resp = ui.button(label);
            if resp.clicked() {
                st.expanded = !st.expanded;
            }
            out.borrow_mut().details_button_rect = Some(resp.rect);
        });

        // -- expanded detail: scrollable text + Copy error --
        if st.expanded {
            ui.label(&st.detail);
            let copy = ui.button("Copy error");
            if copy.clicked() {
                // Same clipboard call as error_strip.rs.
                ui.ctx().copy_text(st.detail.clone());
            }
            out.borrow_mut().copy_button_rect = Some(copy.rect);
        }
    });

    // Extract what actually landed on the clipboard output this frame.
    let mut frame_out = out.into_inner();
    for cmd in &full_output.platform_output.commands {
        if let OutputCommand::CopyText(text) = cmd {
            frame_out.copied_text = Some(text.clone());
        }
    }
    full_output.drop_without_applying_deltas();
    frame_out
}

fn press(pos: Pos2) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events: vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    }
}

fn release(pos: Pos2) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events: vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    }
}

fn empty_input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        ..Default::default()
    }
}

/// Press on one frame, release on the next — the click pattern proven in
/// autocomplete_focus.rs. Returns the release frame's output (clicks and
/// their side effects land there).
fn click_frame(ctx: &Context, st: &mut ErrorPanelState, rect: Rect) -> FrameOut {
    draw_frame(ctx, st, press(rect.center()));
    draw_frame(ctx, st, release(rect.center()))
}

#[test]
fn details_button_toggles_expanded_state_both_ways() {
    let ctx = Context::default();
    let mut st = ErrorPanelState {
        expanded: false,
        detail: "ERROR: syntax error".into(),
    };

    let f0 = draw_frame(&ctx, &mut st, empty_input());
    assert!(!st.expanded);
    assert!(
        f0.copy_button_rect.is_none(),
        "copy button hidden while collapsed"
    );

    let details = f0
        .details_button_rect
        .expect("Details button should render");
    click_frame(&ctx, &mut st, details);
    assert!(st.expanded, "clicking Details should expand the panel");

    let f1 = draw_frame(&ctx, &mut st, empty_input());
    let hide = f1.details_button_rect.expect("Hide button should render");
    click_frame(&ctx, &mut st, hide);
    assert!(
        !st.expanded,
        "clicking Hide should collapse the panel again"
    );
}

#[test]
fn copy_button_pushes_full_multiline_detail_to_clipboard_output() {
    let ctx = Context::default();
    let detail = "ERROR: 42601: syntax error at or near \"on\"\nDETAIL: ...near \"on\"...\nHINT: check the join condition".to_owned();
    let mut st = ErrorPanelState {
        expanded: false,
        detail: detail.clone(),
    };

    // Expand first — the Copy button only exists while open.
    let f0 = draw_frame(&ctx, &mut st, empty_input());
    let details = f0
        .details_button_rect
        .expect("Details button should render");
    click_frame(&ctx, &mut st, details);
    let f1 = draw_frame(&ctx, &mut st, empty_input());
    let copy = f1
        .copy_button_rect
        .expect("Copy button should render while expanded");
    let clicked_out = click_frame(&ctx, &mut st, copy);

    assert_eq!(
        clicked_out.copied_text.as_deref(),
        Some(detail.as_str()),
        "clipboard must receive the full ERROR/DETAIL/HINT block, not the headline"
    );
}
