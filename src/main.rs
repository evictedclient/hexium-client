#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod check;
mod config;
mod ui;

use std::sync::Arc;

use api::ApiClient;

fn main() -> eframe::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--check") {
        attach_parent_console();
        std::process::exit(check::run());
    }
    if arguments.iter().any(|argument| argument == "--help" || argument == "-h") {
        attach_parent_console();
        println!("hexium client");
        println!("  (no arguments)   start the desktop client");
        println!("  --check          hit every read endpoint once and report what parsed");
        println!("  --help           this text");
        return Ok(());
    }

    let config = config::load();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .build()
        .expect("tokio runtime");

    let api = Arc::new(ApiClient::new(
        &config.origin,
        config.session_cookie.as_deref(),
    ));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([config.window_width, config.window_height])
            .with_min_inner_size([1000.0, 640.0])
            .with_title("Hexium"),
        ..Default::default()
    };

    eframe::run_native(
        "hexium-client",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            ui::install_fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, config, api, runtime)))
        }),
    )
}

/// Release builds are GUI-subsystem, so `--check` needs the parent terminal
/// back before it can print anything.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}
