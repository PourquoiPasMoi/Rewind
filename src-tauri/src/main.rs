// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod detect;
mod downloader;
mod library;
mod recorder;
mod schedule;
mod store;
mod tools;
mod tray;
mod updater;
mod winutil;

use recorder::Shared;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use store::Store;
use tauri::{Manager, RunEvent, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::Pending::default())
        .manage(downloader::Downloads::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_local_data_dir()?;
            let default_folder = app
                .path()
                .video_dir()
                .or_else(|_| app.path().home_dir().map(|h| h.join("Videos")))?
                .join("Rewind");
            let store_path = config_dir.join("rewind.json");
            let store = Store::load(&store_path, &default_folder);
            let folder = store.settings.folder();
            let _ = std::fs::create_dir_all(&folder);
            let hidden = store.settings.start_hidden && std::env::args().any(|a| a == "--autostart");
            let paused = store.settings.paused;

            let shared = Arc::new(Shared::new(store, store_path, data_dir.join("bin")));
            let _ = shared.save_store();
            app.asset_protocol_scope().allow_directory(&folder, true)?;
            app.manage(shared.clone());
            tray::build(&handle, paused)?;

            if let Some(window) = app.get_webview_window("main") {
                if !hidden {
                    let _ = window.show();
                }
            }
            tauri::async_runtime::spawn(updater::background(handle.clone(), shared.clone()));
            tauri::async_runtime::spawn(recorder::run(handle, shared));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Close = hide to tray (keeps recording). Never run exit logic here unless asked to.
                api.prevent_close();
                let app = window.app_handle().clone();
                let sh = recorder::store_of(&app);
                let to_tray = sh.store.lock().unwrap().settings.close_to_tray;
                if to_tray {
                    let _ = window.hide();
                } else {
                    tauri::async_runtime::spawn(recorder::shutdown(app, sh));
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::add_channel,
            commands::remove_channel,
            commands::set_channel_enabled,
            commands::set_channel_color,
            commands::set_channel_timers,
            commands::record_now,
            commands::stop_recording,
            commands::check_now,
            commands::set_paused,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::list_tapes,
            commands::delete_tape,
            commands::reveal,
            commands::open_external,
            commands::open_url,
            commands::tools_status,
            commands::install_yt_dlp,
            commands::update_yt_dlp,
            commands::install_ffmpeg,
            commands::disk_info,
            commands::launch_info,
            commands::probe_video,
            commands::start_download,
            commands::list_downloads,
            commands::cancel_download,
            commands::clear_downloads,
            commands::check_update,
            commands::install_update,
            commands::quit_app,
            commands::hide_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Rewind")
        .run(|app, event| {
            if let RunEvent::ExitRequested { api, code, .. } = event {
                // Only our own shutdown (which saves the tapes first) may end the app.
                let sh = recorder::store_of(app);
                if code.is_none() && !sh.quitting.load(Ordering::Relaxed) {
                    api.prevent_exit();
                }
            }
        });
}
