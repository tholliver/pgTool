#![allow(dead_code)]
// Hide the PowerShell/CMD window that Windows spawns next to the GUI in
// release builds; keep it in debug builds so tracing output stays visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod connections;
mod db;
mod query_builder;
mod ui;

use dotenvy::dotenv;

fn main() -> eframe::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("pgqb=debug")
        .init();

    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    let _rt_guard = rt.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("pgTool -- PostgreSQL Browser"),
        ..Default::default()
    };

    eframe::run_native(
        "pgTool",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new(rt)))),
    )
}
