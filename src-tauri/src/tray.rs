//! Tray icon: Rewind keeps watching from here while the window is closed.

use crate::recorder;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "rewind";
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray.png");
const ICON_REC: &[u8] = include_bytes!("../icons/tray-rec.png");

pub struct PauseItem(pub CheckMenuItem<Wry>);

pub fn build(app: &AppHandle, paused: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Rewind", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause watching", true, paused, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Rewind", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &separator, &quit])?;
    app.manage(PauseItem(pause));

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON_IDLE)?)
        .tooltip("Rewind - watching your channels")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "pause" => {
                let sh = recorder::store_of(app);
                let paused = {
                    let mut store = sh.store.lock().unwrap();
                    store.settings.paused = !store.settings.paused;
                    store.settings.paused
                };
                let _ = sh.save_store();
                sync_pause(app, paused);
                recorder::emit_snapshot(app, &sh);
                sh.wake.notify_one();
            }
            "quit" => {
                let app = app.clone();
                let sh = recorder::store_of(&app);
                tauri::async_runtime::spawn(recorder::shutdown(app, sh));
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn sync_pause(app: &AppHandle, paused: bool) {
    if let Some(item) = app.try_state::<PauseItem>() {
        let _ = item.0.set_checked(paused);
    }
}

pub fn set_recording(app: &AppHandle, count: usize) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let icon = if count > 0 { ICON_REC } else { ICON_IDLE };
        if let Ok(image) = Image::from_bytes(icon) {
            let _ = tray.set_icon(Some(image));
        }
        let text = match count {
            0 => "Rewind - watching your channels".to_string(),
            1 => "Rewind - recording 1 stream".to_string(),
            n => format!("Rewind - recording {n} streams"),
        };
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn set_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
