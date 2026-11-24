//! Finds the two helper programs: yt-dlp (grabs the stream) and ffmpeg (writes the file).
//! Node is optional: newer yt-dlp versions use a JavaScript runtime for YouTube.

use crate::winutil::NO_WINDOW;
use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::AsyncWriteExt;

pub const YT_DLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub yt_dlp: Option<PathBuf>,
    pub yt_dlp_version: Option<String>,
    /// The copy in Rewind's own folder (can be updated from the app).
    pub yt_dlp_managed: bool,
    pub ffmpeg: Option<PathBuf>,
    pub ffmpeg_version: Option<String>,
    pub node: Option<PathBuf>,
}

impl Tools {
    pub fn ready(&self) -> bool {
        self.yt_dlp.is_some() && self.ffmpeg.is_some()
    }
}

pub fn detect(bin_dir: &Path) -> Tools {
    let yt_dlp = find("yt-dlp", bin_dir);
    let ffmpeg = find("ffmpeg", bin_dir).or_else(winget_ffmpeg);
    Tools {
        yt_dlp_version: yt_dlp.as_deref().and_then(|p| first_line(p, "--version")),
        yt_dlp_managed: yt_dlp.as_deref().is_some_and(|p| p.starts_with(bin_dir)),
        ffmpeg_version: ffmpeg.as_deref().and_then(|p| first_line(p, "-version")).map(|line| {
            line.strip_prefix("ffmpeg version ")
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or(&line)
                .to_string()
        }),
        node: find("node", bin_dir),
        yt_dlp,
        ffmpeg,
    }
}

fn first_line(exe: &Path, arg: &str) -> Option<String> {
    let out = Command::new(exe).arg(arg).creation_flags(NO_WINDOW).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

fn find(name: &str, bin_dir: &Path) -> Option<PathBuf> {
    let exe = format!("{name}.exe");
    let mut candidates = vec![bin_dir.join(&exe)];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(&exe)));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Microsoft\WinGet\Links").join(&exe));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// winget's Gyan.FFmpeg package is not always on PATH until a new login.
fn winget_ffmpeg() -> Option<PathBuf> {
    let packages = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\WinGet\Packages");
    for package in std::fs::read_dir(packages).ok()?.flatten() {
        if !package.file_name().to_string_lossy().to_ascii_lowercase().contains("ffmpeg") {
            continue;
        }
        for build in std::fs::read_dir(package.path()).ok()?.flatten() {
            let exe = build.path().join(r"bin\ffmpeg.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// Downloads the official yt-dlp.exe into Rewind's own folder.
pub async fn download_yt_dlp(
    http: &reqwest::Client,
    bin_dir: &Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(bin_dir).await.map_err(|e| e.to_string())?;
    let target = bin_dir.join("yt-dlp.exe");
    let tmp = bin_dir.join("yt-dlp.exe.download");
    let mut resp = http
        .get(YT_DLP_URL)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(&tmp).await.map_err(|e| e.to_string())?;
    let mut received = 0u64;
    let mut last_report = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Download failed: {e}"))? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        if received - last_report > 256 * 1024 {
            last_report = received;
            progress(received, total);
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if received < 1024 * 1024 {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err("The download was incomplete. Try again.".into());
    }
    progress(received, total);
    tokio::fs::rename(&tmp, &target).await.map_err(|e| e.to_string())?;
    Ok(target)
}

/// `yt-dlp -U` on Rewind's own copy. Returns yt-dlp's last line ("yt-dlp is up to date ...").
pub fn update_yt_dlp(exe: &Path) -> Result<String, String> {
    let out = Command::new(exe).arg("-U").creation_flags(NO_WINDOW).output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Done").trim().to_string();
    if out.status.success() {
        Ok(last)
    } else {
        Err(last)
    }
}

/// Opens a console window that installs ffmpeg with winget (the user sees and controls it).
pub fn install_ffmpeg_with_winget() -> Result<(), String> {
    Command::new("cmd")
        .args(["/c", "start", "Install FFmpeg", "cmd", "/k", "winget install -e --id Gyan.FFmpeg"])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
