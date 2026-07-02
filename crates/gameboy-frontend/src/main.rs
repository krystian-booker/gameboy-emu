use gameboy_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

mod app;
mod audio;
mod browser;
mod config;
mod input;
mod library;
mod renderer;
mod session;
mod theme;

use app::App;

fn main() -> eframe::Result<()> {
    let initial_size = [720.0, 660.0];

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("CheddyGB")
            .with_inner_size(initial_size)
            .with_min_inner_size([(SCREEN_WIDTH * 3) as f32, (SCREEN_HEIGHT * 3) as f32]),
        ..Default::default()
    };

    eframe::run_native(
        "CheddyGB",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
