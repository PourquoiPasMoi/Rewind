//! Finds the two helper programs: yt-dlp (grabs the stream) and ffmpeg (writes the file).
//! Node is optional: newer yt-dlp versions use a JavaScript runtime for YouTube.

use crate::winutil::NO_WINDOW;
use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
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

/// No byte for this long = the connection is dead; reconnect and resume.
const STALL: Duration = Duration::from_secs(60);
const ATTEMPTS: u32 = 8;

/// Downloads the official yt-dlp.exe into Rewind's own folder.
///
/// Made for slow and unstable lines: there is no total time limit (the shared client's
/// 25 s limit is for tiny live checks only), a stalled or dropped connection reconnects,
/// and every retry continues from the bytes already saved (HTTP Range) instead of restarting.
pub async fn download_yt_dlp(
    http: &reqwest::Client,
    bin_dir: &Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(bin_dir).await.map_err(|e| e.to_string())?;
    let target = bin_dir.join("yt-dlp.exe");
    let tmp = bin_dir.join("yt-dlp.exe.download");
    // A part left by an earlier failed try is continued, not thrown away.
    let mut received = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);
    let mut total: Option<u64> = None;
    let mut last_error = String::new();

    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(4)))).await;
        }
        match download_pass(http, &tmp, &mut received, &mut total, &progress).await {
            Ok(()) => {
                if received < 1024 * 1024 || total.is_some_and(|t| received < t) {
                    last_error = "the file arrived incomplete".into();
                    continue;
                }
                progress(received, total);
                let _ = tokio::fs::remove_file(&target).await;
                tokio::fs::rename(&tmp, &target).await.map_err(|e| e.to_string())?;
                // A resumed file that got mixed up would not start: catch it now, not mid-stream.
                let check = target.clone();
                let works = tokio::task::spawn_blocking(move || first_line(&check, "--version").is_some())
                    .await
                    .unwrap_or(false);
                if !works {
                    let _ = tokio::fs::remove_file(&target).await;
                    return Err(
                        "The downloaded yt-dlp would not start. It may be damaged, or your antivirus blocked it. Press Download to try again."
                            .into(),
                    );
                }
                return Ok(target);
            }
            Err(e) => last_error = e,
        }
    }
    Err(format!(
        "Download stopped at {} MB: {last_error}. Press Download again to continue from there.",
        received / 1_048_576
    ))
}

async fn download_pass(
    http: &reqwest::Client,
    tmp: &Path,
    received: &mut u64,
    total: &mut Option<u64>,
    progress: &impl Fn(u64, Option<u64>),
) -> Result<(), String> {
    let mut req = http.get(YT_DLP_URL).timeout(Duration::from_secs(6 * 3600));
    if *received > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", *received));
    }
    let mut resp = req.send().await.map_err(describe)?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Saved part is broken or from an older release: start over.
        *received = 0;
        let _ = tokio::fs::remove_file(tmp).await;
        return Err("starting over".into());
    }
    let resp_ok = resp.error_for_status_ref().map(|_| ()).map_err(describe);
    resp_ok?;
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed {
        // The server sent the whole file: write it from the start.
        *received = 0;
    }
    if let Some(len) = resp.content_length() {
        *total = Some(len + *received);
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(tmp)
        .await
        .map_err(|e| e.to_string())?;
    let mut last_report = 0u64;
    loop {
        let chunk = match tokio::time::timeout(STALL, resp.chunk()).await {
            Err(_) => return Err("the connection stopped sending data".into()),
            Ok(result) => result.map_err(describe)?,
        };
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        *received += chunk.len() as u64;
        if *received - last_report > 256 * 1024 {
            last_report = *received;
            progress(*received, *total);
        }
    }
    file.flush().await.map_err(|e| e.to_string())
}

fn describe(e: reqwest::Error) -> String {
    if e.is_connect() {
        "no connection to GitHub".into()
    } else if e.is_timeout() {
        "the connection timed out".into()
    } else if e.is_status() {
        format!("GitHub answered {}", e.status().map(|s| s.as_u16()).unwrap_or(0))
    } else {
        "the connection dropped".into()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Network test (about 18 MB): `cargo test resumes_download -- --ignored --nocapture`.
    /// Expects `$TEMP/rewind-dl-test/yt-dlp.exe.download` to hold the first bytes of the file.
    #[tokio::test]
    #[ignore]
    async fn resumes_download() {
        let dir = std::env::temp_dir().join("rewind-dl-test");
        let before = std::fs::metadata(dir.join("yt-dlp.exe.download")).map(|m| m.len()).unwrap_or(0);
        println!("partial file before: {before} bytes");
        let http = reqwest::Client::builder().timeout(Duration::from_secs(25)).build().unwrap();
        let exe = download_yt_dlp(&http, &dir, |got, total| println!("{got} / {total:?}")).await.unwrap();
        println!("version: {:?}", first_line(&exe, "--version"));
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
