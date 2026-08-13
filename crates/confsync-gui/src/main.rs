// Terminalde konsol penceresi açılmasın (Windows dışında etkisiz).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 720.0])
            .with_min_inner_size([820.0, 560.0])
            .with_title("confsync — configuration backup"),
        ..Default::default()
    };

    eframe::run_native(
        "confsync",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )
}
