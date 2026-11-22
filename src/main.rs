// No terminal window in any build; the command-line flags reattach to the
// terminal they were run from.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod api;
mod app;
mod autostart;
mod bot;
mod check;
mod config;
mod icon;
mod instance;
mod notify;
mod play;
mod pricewatch;
mod shortcut;
mod tray;
mod ui;
mod update;
mod values;
mod watch;

use std::sync::Arc;

use api::ApiClient;

fn main() -> eframe::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--check") {
        attach_parent_console();
        std::process::exit(check::run());
    }
    if arguments.iter().any(|argument| argument == "--register") {
        attach_parent_console();
        let mut config = config::load();
        return match shortcut::register(&mut config) {
            Ok(()) => {
                println!("registered as {} (Start menu shortcut \"hexium client\")", shortcut::APP_ID);
                Ok(())
            }
            Err(error) => {
                eprintln!("could not register: {error}");
                std::process::exit(1);
            }
        };
    }
    if arguments.iter().any(|argument| argument == "--help" || argument == "-h") {
        attach_parent_console();
        println!("hexium client");
        println!("  (no arguments)   start the desktop client");
        println!("  --check          hit every read endpoint once and report what parsed");
        println!("  --minimized      start hidden in the tray (what \"Open at sign-in\" uses)");
        println!("  --register       add the Start menu shortcut that gives notifications their name and icon");
        println!("  --help           this text");
        return Ok(());
    }

    update::clean_up();
    // One client at a time: a second launch raises the first and leaves.
    // Started by an update's restart: wait for the old one to close first.
    let claimed = if arguments.iter().any(|argument| argument == "--updated") {
        instance::claim_waiting(std::time::Duration::from_secs(15))
    } else {
        instance::claim()
    };
    if !claimed {
        instance::wake_existing();
        return Ok(());
    }

    let start_minimized = arguments.iter().any(|argument| argument == "--minimized");
    // Keep an existing sign-in entry pointing at this exe.
    autostart::refresh();

    let mut config = config::load();
    // Own name and icon for notifications. Best effort: without it, toasts fall
    // back to PowerShell's id and still show.
    let _ = shortcut::register(&mut config);
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
            .with_title("Hexium")
            .with_icon(Arc::new(eframe::egui::IconData {
                rgba: icon::rgba(64),
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };

    eframe::run_native(
        "hexium-client",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            ui::install_fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, config, api, runtime, start_minimized)))
        }),
    )
}

/// The client is GUI-subsystem, so `--check` and friends need the parent
/// terminal back before they can print anything.
#[cfg(windows)]
fn attach_parent_console() {
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
