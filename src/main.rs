// Release builds are a GUI app with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod app;
mod db;
mod forms;
mod model;
mod money;
mod touch;
mod units;

fn main() {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("the icon is a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Penny Pincher")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([720.0, 440.0])
            .with_icon(icon),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Penny Pincher",
        options,
        Box::new(|creation_context| {
            touch::apply(&creation_context.egui_ctx);
            Ok(Box::new(app::PennyApp::new()?))
        }),
    );
    if let Err(error) = result {
        rfd::MessageDialog::new()
            .set_title("Penny Pincher")
            .set_description(format!("Penny Pincher couldn't start.\n\n{error}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
