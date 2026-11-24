//! Everything the UI can ask the Rust side to do.

use crate::detect;
use crate::library::{self, Tape};
use crate::recorder::{self, Shared, Snapshot};
use crate::store::{Channel, Settings};
use crate::tools::{self, Tools};
use crate::{tray, winutil};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

type R<T> = Result<T, String>;
type Sh<'a> = State<'a, Arc<Shared>>;

#[tauri::command]
pub fn get_snapshot(sh: Sh) -> Snapshot {
    recorder::snapshot(&sh)
}

#[tauri::command]
pub async fn add_channel(app: AppHandle, sh: Sh<'_>, input: String) -> R<Channel> {
    let (platform, handle) = detect::parse_input(&input)?;
    let exists = |sh: &Shared, handle: &str| {
        let id = format!("{}:{}", platform.label(), handle.to_ascii_lowercase());
        sh.store.lock().unwrap().channels.iter().any(|c| c.id == id)
    };
    if exists(&sh, &handle) {
        return Err("That channel is already on your shelf".into());
    }
    let resolved = detect::resolve(&sh.http, platform, &handle).await?;
    if exists(&sh, &resolved.handle) {
        return Err("That channel is already on your shelf".into());
    }
    let channel = {
        let mut store = sh.store.lock().unwrap();
        let channel = Channel {
            id: format!("{}:{}", resolved.platform.label(), resolved.handle.to_ascii_lowercase()),
            platform: resolved.platform,
            handle: resolved.handle,
            name: resolved.name,
            avatar: resolved.avatar,
            enabled: true,
            color: (store.channels.len() % 6) as u8,
            added_at: recorder::now_ms(),
            timers: Vec::new(),
        };
        store.channels.push(channel.clone());
        channel
    };
    sh.save_store()?;
    recorder::check_now(&sh, Some(&channel.id));
    recorder::emit_snapshot(&app, &sh);
    Ok(channel)
}

#[tauri::command]
pub fn remove_channel(app: AppHandle, sh: Sh, id: String) -> R<()> {
    if sh.sessions.lock().unwrap().contains_key(&id) {
        return Err("Stop the recording before removing this channel".into());
    }
    sh.store.lock().unwrap().channels.retain(|c| c.id != id);
    sh.save_store()?;
    recorder::forget(&sh, &id);
    recorder::emit_snapshot(&app, &sh);
    Ok(())
}

#[tauri::command]
pub fn set_channel_enabled(app: AppHandle, sh: Sh, id: String, enabled: bool) -> R<()> {
    {
        let mut store = sh.store.lock().unwrap();
        let channel = store.channels.iter_mut().find(|c| c.id == id).ok_or("Unknown channel")?;
        channel.enabled = enabled;
    }
    sh.save_store()?;
    if enabled {
        recorder::check_now(&sh, Some(&id));
    }
    recorder::emit_snapshot(&app, &sh);
    Ok(())
}

#[tauri::command]
pub fn set_channel_color(app: AppHandle, sh: Sh, id: String, color: u8) -> R<()> {
    {
        let mut store = sh.store.lock().unwrap();
        let channel = store.channels.iter_mut().find(|c| c.id == id).ok_or("Unknown channel")?;
        channel.color = color % 6;
    }
    sh.save_store()?;
    recorder::emit_snapshot(&app, &sh);
    Ok(())
}

#[tauri::command]
pub fn set_channel_timers(app: AppHandle, sh: Sh, id: String, timers: Vec<crate::store::Timer>) -> R<()> {
    if timers.len() > 8 {
        return Err("A channel can have up to 8 timer programs".into());
    }
    for t in &timers {
        if t.days & 0x7f == 0 {
            return Err("Pick at least one day for each timer".into());
        }
        if t.start >= 1440 || t.end >= 1440 {
            return Err("That time is not valid".into());
        }
    }
    {
        let mut store = sh.store.lock().unwrap();
        let channel = store.channels.iter_mut().find(|c| c.id == id).ok_or("Unknown channel")?;
        channel.timers = timers;
    }
    sh.save_store()?;
    // Look right away if the new window is already open.
    recorder::check_now(&sh, Some(&id));
    recorder::emit_snapshot(&app, &sh);
    Ok(())
}

#[tauri::command]
pub fn record_now(sh: Sh, id: String) {
    recorder::record_now(&sh, &id);
}

#[tauri::command]
pub fn stop_recording(app: AppHandle, sh: Sh, id: String) {
    recorder::stop(&app, &sh, &id);
}

#[tauri::command]
pub fn check_now(sh: Sh, id: Option<String>) {
    recorder::check_now(&sh, id.as_deref());
}

#[tauri::command]
pub fn set_paused(app: AppHandle, sh: Sh, paused: bool) -> R<()> {
    sh.store.lock().unwrap().settings.paused = paused;
    sh.save_store()?;
    tray::sync_pause(&app, paused);
    recorder::emit_snapshot(&app, &sh);
    sh.wake.notify_one();
    Ok(())
}

#[tauri::command]
pub fn get_settings(sh: Sh) -> Settings {
    sh.store.lock().unwrap().settings.clone()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, sh: Sh, mut settings: Settings) -> R<Settings> {
    let old = sh.store.lock().unwrap().settings.clone();
    settings.sanitize(&old.folder());
    if settings.folder != old.folder {
        let folder = settings.folder();
        std::fs::create_dir_all(&folder).map_err(|e| format!("Cannot use that folder: {e}"))?;
        app.asset_protocol_scope().allow_directory(&folder, true).map_err(|e| e.to_string())?;
    }
    if settings.autostart != old.autostart {
        winutil::set_autostart(settings.autostart)?;
    }
    sh.store.lock().unwrap().settings = settings.clone();
    sh.save_store()?;
    if settings.paused != old.paused {
        tray::sync_pause(&app, settings.paused);
    }
    if settings.folder != old.folder {
        let _ = app.emit("tapes-changed", ());
    }
    if settings.keep_days != old.keep_days && settings.keep_days > 0 {
        if library::purge_older(&settings.folder(), settings.keep_days) > 0 {
            let _ = app.emit("tapes-changed", ());
        }
    }
    recorder::emit_snapshot(&app, &sh);
    sh.wake.notify_one();
    Ok(settings)
}

#[tauri::command]
pub async fn pick_folder(window: WebviewWindow) -> R<Option<String>> {
    // A fresh thread: rfd dialogs fail silently on a thread already in multithreaded COM.
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::spawn(move || {
            rfd::FileDialog::new()
                .set_title("Where should Rewind keep your tapes?")
                .set_parent(&window)
                .pick_folder()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .join()
        .map_err(|_| "The folder dialog failed".to_string())
    })
    .await
    .map_err(|_| "The folder dialog failed".to_string())?
}

#[tauri::command]
pub async fn list_tapes(sh: Sh<'_>) -> R<Vec<Tape>> {
    let folder = sh.store.lock().unwrap().settings.folder();
    tauri::async_runtime::spawn_blocking(move || library::list(&folder)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_tape(app: AppHandle, sh: Sh, path: String) -> R<()> {
    let folder = sh.store.lock().unwrap().settings.folder();
    library::delete(&folder, &PathBuf::from(path))?;
    let _ = app.emit("tapes-changed", ());
    Ok(())
}

#[tauri::command]
pub fn reveal(sh: Sh, path: Option<String>) -> R<()> {
    let folder = sh.store.lock().unwrap().settings.folder();
    match path {
        Some(path) => {
            let path = PathBuf::from(path);
            if !library::inside(&folder, &path) {
                return Err("That file is not in the recordings folder".into());
            }
            winutil::reveal(&path);
            Ok(())
        }
        None => {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            winutil::shell_open(&folder.to_string_lossy())
        }
    }
}

#[tauri::command]
pub fn open_external(sh: Sh, path: String) -> R<()> {
    let folder = sh.store.lock().unwrap().settings.folder();
    let path = PathBuf::from(&path);
    if !library::inside(&folder, &path) {
        return Err("That file is not in the recordings folder".into());
    }
    winutil::shell_open(&path.to_string_lossy())
}

#[tauri::command]
pub fn open_url(url: String) -> R<()> {
    let allowed = ["https://www.twitch.tv/", "https://www.youtube.com/", "https://github.com/yt-dlp/", "https://www.gyan.dev/"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return Err("Rewind only opens Twitch and YouTube links".into());
    }
    winutil::shell_open(&url)
}

#[tauri::command]
pub async fn tools_status(app: AppHandle, sh: Sh<'_>) -> R<Tools> {
    let bin = sh.bin_dir.clone();
    let tools = tauri::async_runtime::spawn_blocking(move || tools::detect(&bin)).await.map_err(|e| e.to_string())?;
    *sh.tools.lock().unwrap() = tools.clone();
    recorder::emit_snapshot(&app, &sh);
    Ok(tools)
}

#[tauri::command]
pub async fn install_yt_dlp(app: AppHandle, sh: Sh<'_>) -> R<Tools> {
    let progress_app = app.clone();
    tools::download_yt_dlp(&sh.http, &sh.bin_dir, move |received, total| {
        let _ = progress_app.emit("tool-progress", serde_json::json!({ "received": received, "total": total }));
    })
    .await?;
    tools_status(app, sh).await
}

#[tauri::command]
pub async fn update_yt_dlp(app: AppHandle, sh: Sh<'_>) -> R<String> {
    let exe = sh.bin_dir.join("yt-dlp.exe");
    if !exe.is_file() {
        return Err("Only the copy Rewind downloaded can update itself".into());
    }
    let message = tauri::async_runtime::spawn_blocking(move || tools::update_yt_dlp(&exe)).await.map_err(|e| e.to_string())??;
    let _ = tools_status(app, sh).await;
    Ok(message)
}

#[tauri::command]
pub fn install_ffmpeg() -> R<()> {
    tools::install_ffmpeg_with_winget()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    free: u64,
    total: u64,
}

#[tauri::command]
pub fn disk_info(sh: Sh) -> Option<Disk> {
    use std::os::windows::ffi::OsStrExt;
    let folder = sh.store.lock().unwrap().settings.folder();
    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut())
    };
    (ok != 0).then_some(Disk { free, total })
}

#[tauri::command]
pub fn launch_info() -> serde_json::Value {
    serde_json::json!({ "version": env!("CARGO_PKG_VERSION") })
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> R<Option<crate::updater::UpdateInfo>> {
    crate::updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle, sh: Sh<'_>, force: bool) -> R<()> {
    crate::updater::install(app, sh.inner().clone(), force).await
}

#[tauri::command]
pub fn quit_app(app: AppHandle, sh: Sh) {
    tauri::async_runtime::spawn(recorder::shutdown(app.clone(), sh.inner().clone()));
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}
