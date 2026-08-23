pub mod state;

use state::AppState;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::ui;

pub struct App {
    pub state: AppState,
    pub rt: Arc<Runtime>,
    pub theme: ui::Theme,
    initialized: bool,
}

impl App {
    pub fn new(rt: Runtime) -> Self {
        Self {
            state: AppState::new(),
            rt: Arc::new(rt),
            theme: ui::DARK_ZINC_PURPLE,
            initialized: false,
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let theme = &self.theme;
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(theme.text);
        visuals.widgets.noninteractive.bg_fill = theme.panel;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, theme.text_muted);
        visuals.widgets.inactive.bg_fill = theme.surface;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.text);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
        visuals.widgets.hovered.bg_fill = theme.accent_hover;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.widgets.active.bg_fill = theme.accent;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        visuals.selection.bg_fill = theme.accent_muted;
        visuals.selection.stroke = egui::Stroke::new(1.0, theme.accent);
        visuals.extreme_bg_color = theme.bg;
        visuals.faint_bg_color = theme.panel;
        visuals.striped = false;
        visuals.slider_trailing_fill = true;
        // scrollbar styling
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
        ctx.set_visuals(visuals);

        // font overrides
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.interaction.selectable_labels = false;
        ctx.set_style(style);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initialized {
            self.apply_theme(ctx);
            self.initialized = true;
        }

        // Keyboard shortcuts — must run BEFORE ui::draw so that consumed
        // keys (Alt+Enter) are removed from the input and never reach the
        // SQL editor as an extra newline.
        ctx.input_mut(|i| {
            // Only Alt+Enter (or F5) executes; a bare Enter is left alone
            // so the SQL editor can insert a normal newline.
            let run = i.consume_key(egui::Modifiers::ALT, egui::Key::Enter)
                || i.key_pressed(egui::Key::F5);
            if run {
                self.state.run_query();
            }
            if i.key_pressed(egui::Key::Escape) {
                self.state.query.result = None;
                self.state.query.status = "Browsing".into();
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                match self.state.query.focus {
                    state::Focus::Schemas => {
                        // next schema
                        let len = self.state.visible_schemas_len();
                        if len > 0 {
                            self.state.query.schema_idx =
                                (self.state.query.schema_idx + 1).min(len - 1);
                            self.state.query.table_idx = 0;
                            self.state.update_sql();
                        }
                    }
                    state::Focus::Tables => {
                        let len = self
                            .state
                            .selected_schema()
                            .map(|s| s.tables.len())
                            .unwrap_or(0);
                        if len > 0 {
                            self.state.query.table_idx =
                                (self.state.query.table_idx + 1).min(len - 1);
                            self.state.update_sql();
                        }
                    }
                }
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                match self.state.query.focus {
                    state::Focus::Schemas => {
                        if self.state.visible_schemas_len() > 0 {
                            self.state.query.schema_idx =
                                self.state.query.schema_idx.saturating_sub(1);
                            self.state.query.table_idx = 0;
                            self.state.update_sql();
                        }
                    }
                    state::Focus::Tables => {
                        if self
                            .state
                            .selected_schema()
                            .map(|s| s.tables.len())
                            .unwrap_or(0)
                            > 0
                        {
                            self.state.query.table_idx =
                                self.state.query.table_idx.saturating_sub(1);
                            self.state.update_sql();
                        }
                    }
                }
            }
        });

        if self.state.poll_promises() {
            ctx.request_repaint();
        }
        if self.state.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        ui::draw(ctx, &mut self.state, &self.theme);
    }
}
