mod ai;
mod config;
mod converter;
mod music;
mod organizer;
mod persistence;
mod platform;
mod sfx;
mod state;
mod tools;
mod ui;
mod voice;
mod weather;

use eframe::egui;
use ui::RustyDeskApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([450.0, 650.0])
            .with_min_inner_size([64.0, 64.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_title("Rusty-Desk"),
        ..Default::default()
    };

    eframe::run_native(
        "Rusty-Desk",
        options,
        Box::new(|cc| Ok(Box::new(RustyDeskApp::new(cc)))),
    )
}
