//! Native entry point. All device I/O runs on a background worker thread.

use eightbitdo_gui::app::App;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 760.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title("8BitDo Ultimate 2 - Read-only"),
        ..Default::default()
    };

    eframe::run_native(
        "8BitDo Ultimate 2 - Read-only",
        options,
        Box::new(|creation| Ok(Box::new(App::new(creation)))),
    )
}
