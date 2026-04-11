//! Praetor — direct manual-control Tauri client for Hivemind drones.
//!
//! See `praetor/README.md` for the design overview and the four-phase
//! delivery plan.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use praetor_lib::config::Config;
use praetor_lib::gamepad;
use praetor_lib::safety;
use praetor_lib::state::AppState;
use praetor_lib::tauri_commands;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
    init_tracing();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("praetor: failed to load config: {e}");
            // Fall back to defaults so the UI still comes up — the operator
            // can edit praetor.toml and reconnect from the UI.
            Config::default()
        }
    };
    tracing::info!(?config, "loaded praetor config");

    let app_state = AppState::new(config);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .setup(move |app| {
            let handle = Arc::new(app.handle().clone());

            // Spawn the background tasks that don't depend on a live MAVLink
            // connection. The MAVLink tasks themselves start when the
            // operator hits "Connect" in the UI.
            tauri::async_runtime::spawn(gamepad::run_poller_task(app_state.clone()));
            tauri::async_runtime::spawn(safety::run_safety_loop(app_state.clone()));
            tauri::async_runtime::spawn(tauri_commands::run_event_emitter(
                Arc::clone(&handle),
                app_state.clone(),
            ));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tauri_commands::connect,
            tauri_commands::disconnect,
            tauri_commands::begin_arming,
            tauri_commands::cancel_arming,
            tauri_commands::emergency_stop,
            tauri_commands::takeoff,
            tauri_commands::land,
            tauri_commands::return_to_launch,
            tauri_commands::cycle_mode,
            tauri_commands::list_serial_ports,
            tauri_commands::get_config,
        ])
        .run(tauri::generate_context!())
        .expect("praetor: fatal tauri runtime error");
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("praetor=info,warn"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}
