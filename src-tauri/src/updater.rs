//! Built-in updates from GitHub Releases.
//!
//! Rewind reads `latest.json` from the newest release (about 1 KB). An update is only
//! installed if its installer is signed with Rewind's private key (checked against the
//! public key in tauri.conf.json), so nobody else can push an update.
//! Order: download while still recording, then save open tapes, then install.

use crate::recorder::{self, SessionState, Shared};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct Pending(pub Mutex<Option<Update>>);

fn friendly(e: tauri_plugin_updater::Error) -> String {
    let text = e.to_string();
    let lower = text.to_ascii_lowercase();
    if lower.contains("signature") {
        "The update's signature did not match, so it was not installed".into()
    } else if lower.contains("network") || lower.contains("connect") || lower.contains("dns") || lower.contains("request") {
        "Could not reach GitHub to check for updates".into()
    } else {
        format!("Update check failed: {text}")
    }
}

pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app.updater().map_err(friendly)?.check().await.map_err(friendly)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
    });
    *app.state::<Pending>().0.lock().unwrap() = update;
    if let Some(info) = &info {
        let _ = app.emit("update-available", info.clone());
    }
    Ok(info)
}

/// Checks shortly after start, then every 6 hours, when "Check for updates" is on.
pub async fn background(app: AppHandle, sh: Arc<Shared>) {
    tokio::time::sleep(Duration::from_secs(20)).await;
    loop {
        let enabled = sh.store.lock().unwrap().settings.auto_update;
        if enabled {
            // Offline or GitHub down: try again next round, nothing to tell the user.
            let _ = check(&app).await;
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}

/// Returns Err("recording") when tapes are being recorded and `force` is false,
/// so the UI can ask first.
pub async fn install(app: AppHandle, sh: Arc<Shared>, force: bool) -> Result<(), String> {
    let busy = sh.sessions_in(&[SessionState::Recording, SessionState::Holding]);
    if busy > 0 && !force {
        return Err("recording".into());
    }
    let pending = app.state::<Pending>().0.lock().unwrap().take();
    let update = match pending {
        Some(update) => update,
        None => {
            check(&app).await?;
            app.state::<Pending>().0.lock().unwrap().take().ok_or("Rewind is already up to date")?
        }
    };

    let mut received = 0u64;
    let progress_app = app.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                let _ = progress_app.emit("update-progress", serde_json::json!({ "received": received, "total": total }));
            },
            || {},
        )
        .await
        .map_err(friendly)?;

    // Downloaded and verified: now stop recorders and save every open tape.
    let _ = app.emit("update-progress", serde_json::json!({ "stage": "saving" }));
    recorder::save_all(&app, &sh).await;
    let _ = app.emit("update-progress", serde_json::json!({ "stage": "installing" }));
    if let Err(e) = update.install(bytes) {
        // Keep working on the current version.
        sh.quitting.store(false, std::sync::atomic::Ordering::Relaxed);
        tauri::async_runtime::spawn(recorder::run(app.clone(), sh.clone()));
        return Err(friendly(e));
    }
    // The installer replaces the exe; start the new version.
    app.restart();
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    /// Network test (~3 MB): checks the published release exactly like the updater does.
    /// `cargo test published_update_verifies -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn published_update_verifies() {
        let b64 = base64::engine::general_purpose::STANDARD;
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pubkey = String::from_utf8(b64.decode(conf["plugins"]["updater"]["pubkey"].as_str().unwrap()).unwrap()).unwrap();
        let endpoint = conf["plugins"]["updater"]["endpoints"][0].as_str().unwrap();
        let latest: serde_json::Value = reqwest::get(endpoint).await.unwrap().json().await.unwrap();
        let platform = &latest["platforms"]["windows-x86_64"];
        let url = platform["url"].as_str().unwrap();
        let signature = String::from_utf8(b64.decode(platform["signature"].as_str().unwrap()).unwrap()).unwrap();
        let installer = reqwest::get(url).await.unwrap().bytes().await.unwrap();
        println!("latest.json says {} -> {} ({} bytes)", latest["version"], url, installer.len());
        let key = minisign_verify::PublicKey::decode(&pubkey).unwrap();
        let sig = minisign_verify::Signature::decode(&signature).unwrap();
        key.verify(&installer, &sig, true).expect("signature must verify");
        println!("signature OK");
    }
}
