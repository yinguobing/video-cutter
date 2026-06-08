// Allow dead code during development
#![allow(dead_code)]

mod app;
mod embed;
mod export;
mod player;
mod types;

use app::DnClipApp;
use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting dnclip v{}", env!("CARGO_PKG_VERSION"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Video Cutter")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dnclip",
        options,
        Box::new(|_cc| Ok(Box::new(DnClipApp::default()))),
    )
}
