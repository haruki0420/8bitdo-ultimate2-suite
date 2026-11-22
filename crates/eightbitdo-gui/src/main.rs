//! Native entry point. All device I/O runs on a background worker thread.

use eightbitdo_gui::app::App;
use eightbitdo_gui::i18n::{t, Key, Lang};

fn main() -> eframe::Result {
    // The window title is not part of the egui tree, so it is set here with the
    // default language. The in-app language switch covers everything else.
    let title = t(Lang::Ja, Key::WindowTitle);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 800.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title(title),
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|creation| Ok(Box::new(App::new(creation)))),
    )
}
