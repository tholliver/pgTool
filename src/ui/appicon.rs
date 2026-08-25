const ICON_BIN: &[u8] = include_bytes!("../../assets/icon.rgba");

fn dims(bytes: &[u8]) -> (usize, usize) {
    let w = u32::from_le_bytes(bytes[0..4].try_into().expect("icon header width"));
    let h = u32::from_le_bytes(bytes[4..8].try_into().expect("icon header height"));
    (w as usize, h as usize)
}

pub fn icon_data() -> egui::IconData {
    let (w, h) = dims(ICON_BIN);
    egui::IconData {
        rgba: ICON_BIN[8..].to_vec(),
        width: w as u32,
        height: h as u32,
    }
}

pub fn load_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let (w, h) = dims(ICON_BIN);
    ctx.load_texture(
        "pgqb_app_icon",
        egui::ColorImage::from_rgba_unmultiplied([w, h], &ICON_BIN[8..]),
        egui::TextureOptions::LINEAR,
    )
}
