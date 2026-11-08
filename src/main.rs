//! Lumi desktop entry point.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")] // no console window on Windows

fn main() {
    let reminders = std::env::args().any(|a| a == "--reminders");
    // a backup chosen in Settings is swapped in before the database opens
    let restored = !reminders && match lumi::backup::finish_pending_restore(&lumi::db::data_dir()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Lumi couldn't finish restoring the backup: {e}");
            false
        }
    };
    let db = match lumi::db::Db::open() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Lumi couldn't open its data folder ({}): {e}", lumi::db::data_dir().display());
            std::process::exit(1);
        }
    };
    if restored {
        db.set("restored_notice", &true);
    }
    if reminders {
        lumi::reminders::run_daemon(&db); // background daily-reminder helper started at login
        return;
    }
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png")).ok().map(|i| {
        let i = i.to_rgba8();
        egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Lumi")
        .with_app_id("lumi") // matches lumi.desktop so the taskbar shows Lumi's icon
        .with_inner_size([1360.0, 900.0])
        .with_min_inner_size([760.0, 560.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    if let Err(e) = eframe::run_native("Lumi", opts, Box::new(move |cc| Ok(Box::new(lumi::ui::App::new(cc, db))))) {
        eprintln!("Lumi couldn't start its window: {e}\nIf this computer has no OpenGL driver, try: LIBGL_ALWAYS_SOFTWARE=1 lumi");
        std::process::exit(1);
    }
}
