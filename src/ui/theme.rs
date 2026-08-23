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
};

impl Theme {
    pub fn accent_button(&self, label: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(label)
                .color(egui::Color32::WHITE)
                .strong(),
        )
        .fill(self.accent)
        .rounding(6.0)
    }

    pub fn surface_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.surface)
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .stroke(egui::Stroke::new(1.0, self.border))
    }

    pub fn panel_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.panel)
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
    }
}
