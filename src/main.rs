// Release builds are a GUI app with no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod app;
mod converter;
mod db;
mod forms;
mod model;
mod money;
mod touch;
mod units;
mod update;

fn main() {
    // A release build has no console, so a crash would otherwise vanish without a trace.
    std::panic::set_hook(Box::new(|info| {
        let details = format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture());
        report_failure("Penny Pincher stopped because of an error.", info.payload_as_str().unwrap_or_default(), &details);
    }));

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
        report_failure("Penny Pincher couldn't start.", &error.to_string(), &format!("{error:?}"));
    }
}

/// Shows `summary` and `reason`, and saves `details` to a log file.
fn report_failure(summary: &str, reason: &str, details: &str) {
    let log = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Penny Pincher")
        .join("penny-pincher.log");
    let entry = format!(
        "{} version {}\n{summary}\n{details}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        env!("CARGO_PKG_VERSION")
    );
    let saved = append_to_file(&log, &entry).is_ok();

    let mut message = format!("{summary}\n\n{reason}");
    if saved {
        message.push_str(&format!("\n\nDetails are in {}", log.display()));
    }
    rfd::MessageDialog::new()
        .set_title("Penny Pincher")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

fn append_to_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(text.as_bytes())
}
