pub struct Theme {
    pub bg: egui::Color32,
    pub panel: egui::Color32,
    pub surface: egui::Color32,
    pub border: egui::Color32,
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub accent: egui::Color32,
    pub accent_hover: egui::Color32,
    pub accent_muted: egui::Color32,
    pub success: egui::Color32,
    pub danger: egui::Color32,
    // -- SQL syntax highlighting --
    pub sql_keyword: egui::Color32,
    pub sql_string: egui::Color32,
    pub sql_number: egui::Color32,
    pub sql_identifier: egui::Color32,
    pub sql_comment: egui::Color32,
}

pub const DARK_ZINC_PURPLE: Theme = Theme {
    bg: egui::Color32::from_rgb(0x09, 0x09, 0x0b),
    panel: egui::Color32::from_rgb(0x18, 0x18, 0x1b),
    surface: egui::Color32::from_rgb(0x27, 0x27, 0x2a),
    border: egui::Color32::from_rgb(0x3f, 0x3f, 0x46),
    text: egui::Color32::from_rgb(0xfa, 0xfa, 0xfa),
    text_muted: egui::Color32::from_rgb(0xa1, 0xa1, 0xaa),
    accent: egui::Color32::from_rgb(0x7c, 0x3a, 0xed),
    accent_hover: egui::Color32::from_rgb(0x8b, 0x5c, 0xf6),
    accent_muted: egui::Color32::from_rgb(0x2e, 0x1f, 0x47),
    success: egui::Color32::from_rgb(0x22, 0xc5, 0x5e),
    danger: egui::Color32::from_rgb(0xef, 0x44, 0x44),
    sql_keyword: egui::Color32::from_rgb(0xc5, 0x86, 0xc0),
    sql_string: egui::Color32::from_rgb(0xce, 0x91, 0x78),
    sql_number: egui::Color32::from_rgb(0xb5, 0xce, 0xa8),
    sql_identifier: egui::Color32::from_rgb(0x9c, 0xdc, 0xfe),
    sql_comment: egui::Color32::from_rgb(0x6a, 0x99, 0x55),
};

impl Theme {
    pub fn accent_button(&self, label: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(label)
                .color(egui::Color32::WHITE)
                .strong(),
        )
        .fill(self.accent)
        .corner_radius(6)
    }

    pub fn surface_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.surface)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .stroke(egui::Stroke::new(1.0, self.border))
    }

    pub fn panel_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.panel)
            .inner_margin(egui::Margin::symmetric(8, 4))
    }
}
