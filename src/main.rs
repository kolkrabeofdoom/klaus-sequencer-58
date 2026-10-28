#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use klaus_sequencer_58::app::KlausApp;

fn main() -> eframe::Result {
    let promo_capture = std::env::var_os("KLAUS_PROMO_CAPTURE").is_some();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("K.L.A.U.S. Sequencer 58")
            .with_inner_size(if promo_capture {
                [1440.0, 1120.0]
            } else {
                [1440.0, 900.0]
            })
            .with_min_inner_size([1024.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "K.L.A.U.S. Sequencer 58",
        options,
        Box::new(|cc| Ok(Box::new(KlausApp::new(cc)))),
    )
}
