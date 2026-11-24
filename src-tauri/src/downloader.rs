//! Download tab: save a YouTube video (or anything yt-dlp supports) in the quality you pick.
//!
//! `probe` asks yt-dlp for the video's formats (`-J`) and turns them into simple choices
//! ("1080p 60 fps", "Sound only") with a size estimate. `start` runs yt-dlp with a format
//! selector for that choice; progress comes from `--progress-template` lines on stdout.

use crate::library::{self, TapeMeta};
use crate::recorder::{now_ms, toast, Shared};
use crate::tools::Tools;
use crate::winutil::{self, NO_WINDOW};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quality {
    /// "2160p60", "1080p", "audio".
    pub id: String,
    pub height: u32,
    pub fps: u32,
    /// Estimated total bytes (picture + sound), when YouTube tells.
    pub size: Option<u64>,
    /// "AV1", "VP9", "H.264".
    pub codec: Option<String>,
    pub hdr: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub url: String,
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
    pub site: String,
    pub qualities: Vec<Quality>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Downloading,
    Joining,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub channel: String,
    pub thumbnail: Option<String>,
    pub quality: String,
    pub state: JobState,
    /// 0..1 over the whole job.
    pub progress: f64,
    pub speed: Option<f64>,
    pub eta: Option<u64>,
    pub size: Option<u64>,
    pub file: Option<String>,
    pub error: Option<String>,
    pub started_at: i64,
    #[serde(skip)]
    pid: Option<u32>,
    #[serde(skip)]
    video_id: String,
    #[serde(skip)]
    dir: PathBuf,
}

#[derive(Default)]
pub struct Downloads {
    jobs: Mutex<Vec<Job>>,
    probes: Mutex<HashMap<String, VideoInfo>>,
    next: AtomicU64,
}

fn emit(app: &AppHandle) {
    let jobs = app.state::<Downloads>().jobs.lock().unwrap().clone();
    let _ = app.emit("downloads", jobs);
}

pub fn list(app: &AppHandle) -> Vec<Job> {
    app.state::<Downloads>().jobs.lock().unwrap().clone()
}

fn codec_name(vcodec: &str) -> Option<String> {
    let v = vcodec.to_ascii_lowercase();
    Some(
        if v.starts_with("av01") {
            "AV1"
        } else if v.starts_with("vp09") || v.starts_with("vp9") {
            "VP9"
        } else if v.starts_with("avc") || v.starts_with("h264") {
            "H.264"
        } else {
            return None;
        }
        .to_string(),
    )
}

fn yt_dlp_base(tools: &Tools) -> Result<tokio::process::Command, String> {
    let exe = tools.yt_dlp.clone().ok_or("yt-dlp is missing. Get it in Setup first.")?;
    let mut cmd = tokio::process::Command::new(exe);
    if let Some(node) = &tools.node {
        cmd.arg("--js-runtimes").arg(format!("node:{}", node.display()));
    }
    if let Some(ffmpeg) = &tools.ffmpeg {
        cmd.arg("--ffmpeg-location").arg(ffmpeg);
    }
    cmd.creation_flags(NO_WINDOW).stdin(Stdio::null()).kill_on_drop(true);
    Ok(cmd)
}

fn explain(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|l| l.contains("ERROR")).or_else(|| stderr.lines().last()).unwrap_or("");
    let lower = line.to_ascii_lowercase();
    if lower.contains("unsupported url") {
        "Rewind can't download from that link".into()
    } else if lower.contains("private video") {
        "This video is private".into()
    } else if lower.contains("sign in") || lower.contains("age") {
        "YouTube wants a signed-in viewer for this video (age limit or region)".into()
    } else if lower.contains("members") {
        "This video is for channel members only".into()
    } else if lower.contains("unavailable") {
        "This video is not available".into()
    } else if lower.contains("unable to download") || lower.contains("timed out") || lower.contains("connection") {
        "The connection dropped. Try again.".into()
    } else if line.is_empty() {
        "yt-dlp stopped without saying why".into()
    } else {
        line.replace("ERROR: ", "").chars().take(180).collect()
    }
}

pub async fn probe(app: &AppHandle, sh: &Arc<Shared>, url: String) -> Result<VideoInfo, String> {
    let url = url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Paste a full video link (https://...)".into());
    }
    let tools = sh.tools.lock().unwrap().clone();
    let mut cmd = yt_dlp_base(&tools)?;
    cmd.args(["-J", "--no-playlist", "--no-warnings"]).arg(&url).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = tokio::time::timeout(Duration::from_secs(90), cmd.output())
        .await
        .map_err(|_| "Looking up the video took too long. Try again.".to_string())?
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    if !out.status.success() {
        return Err(explain(&String::from_utf8_lossy(&out.stderr)));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "yt-dlp sent something unexpected".to_string())?;
    let info = parse_info(&url, &v)?;
    app.state::<Downloads>().probes.lock().unwrap().insert(url, info.clone());
    Ok(info)
}

/// yt-dlp's `-J` JSON -> title, channel and the quality choices.
fn parse_info(url: &str, v: &serde_json::Value) -> Result<VideoInfo, String> {
    let url = url.to_string();
    if v["is_live"].as_bool() == Some(true) {
        return Err("This video is live right now. Add the channel on the Channels tab to record it.".into());
    }
    if v["_type"].as_str() == Some("playlist") {
        return Err("That is a playlist. Paste the link of one video.".into());
    }

    let formats = v["formats"].as_array().cloned().unwrap_or_default();
    let duration = v["duration"].as_f64().unwrap_or(0.0);
    // Exact size when YouTube gives it, else bitrate (kbit/s) x length.
    let size_of = |f: &serde_json::Value| {
        f["filesize"]
            .as_u64()
            .or_else(|| f["filesize_approx"].as_u64())
            .or_else(|| f["tbr"].as_f64().filter(|_| duration > 0.0).map(|kbps| (kbps * 125.0 * duration) as u64))
    };
    // Best sound (for the size estimate and the sound-only choice).
    let audio = formats
        .iter()
        .filter(|f| f["vcodec"].as_str() == Some("none") && f["acodec"].as_str().is_some_and(|a| a != "none"))
        .max_by(|a, b| a["abr"].as_f64().unwrap_or(0.0).total_cmp(&b["abr"].as_f64().unwrap_or(0.0)));
    let audio_size = audio.and_then(size_of);

    // One choice per height + frame rate. The estimate comes from the stream yt-dlp would
    // pick: SDR before HDR, a normal download (https) before a streaming copy (m3u8),
    // then the highest bitrate.
    type Rank = (bool, bool, f64);
    let mut groups: HashMap<(u32, u32), (Rank, Option<u64>, Option<String>, bool)> = HashMap::new();
    for f in &formats {
        let Some(vcodec) = f["vcodec"].as_str().filter(|c| *c != "none") else { continue };
        let Some(height) = f["height"].as_u64().map(|h| h as u32).filter(|h| *h >= 100) else { continue };
        let fps_raw = f["fps"].as_f64().unwrap_or(30.0);
        let fps = if fps_raw > 31.0 { fps_raw.round() as u32 } else { 30 };
        let hdr = f["dynamic_range"].as_str().is_some_and(|d| d != "SDR");
        let https = f["protocol"].as_str().is_some_and(|p| p.starts_with("http") && !p.contains("m3u8"));
        let rank: Rank = (!hdr, https, f["tbr"].as_f64().unwrap_or(0.0));
        let video_only = f["acodec"].as_str() == Some("none");
        let size = size_of(f).map(|s| if video_only { s + audio_size.unwrap_or(0) } else { s });
        let better = |old: &Rank| (rank.0, rank.1) > (old.0, old.1) || ((rank.0, rank.1) == (old.0, old.1) && rank.2 > old.2);
        match groups.get(&(height, fps)) {
            Some((old, ..)) if !better(old) => {}
            _ => {
                groups.insert((height, fps), (rank, size, codec_name(vcodec), hdr));
            }
        }
    }
    let mut qualities: Vec<Quality> = groups
        .into_iter()
        .map(|((height, fps), (_, size, codec, hdr))| Quality {
            id: if fps > 30 { format!("{height}p{fps}") } else { format!("{height}p") },
            height,
            fps,
            size,
            codec,
            hdr,
        })
        .collect();
    qualities.sort_by(|a, b| b.height.cmp(&a.height).then(b.fps.cmp(&a.fps)));
    if audio.is_some() || qualities.is_empty() {
        qualities.push(Quality { id: "audio".into(), height: 0, fps: 0, size: audio_size, codec: None, hdr: false });
    }

    Ok(VideoInfo {
        url,
        id: v["id"].as_str().unwrap_or("video").to_string(),
        title: v["title"].as_str().unwrap_or("Video").to_string(),
        channel: v["channel"].as_str().or_else(|| v["uploader"].as_str()).unwrap_or("").to_string(),
        duration: v["duration"].as_f64().map(|d| d as u64),
        thumbnail: v["thumbnail"].as_str().map(str::to_string),
        site: v["extractor_key"].as_str().unwrap_or("").to_string(),
        qualities,
    })
}

fn selector(q: &Quality) -> String {
    if q.id == "audio" {
        return "ba[ext=m4a]/ba/b".into();
    }
    let h = q.height;
    let fps = if q.fps > 30 { "[fps>30]" } else { "[fps<=30]" };
    // SDR picture first (plays everywhere), then any; M4A sound first (plays in every Windows
    // player), then any; then a combined stream of that height; then the closest below.
    format!(
        "bv*[height={h}]{fps}[dynamic_range=SDR]+ba[ext=m4a]/bv*[height={h}]{fps}[dynamic_range=SDR]+ba/bv*[height={h}]{fps}+ba[ext=m4a]/bv*[height={h}]{fps}+ba/b[height={h}]/bv*[height<={h}]+ba[ext=m4a]/bv*[height<={h}]+ba/b"
    )
}

pub fn quality_label(q: &Quality) -> String {
    if q.id == "audio" {
        "Sound only (MP3)".into()
    } else if q.fps > 30 {
        format!("{}p {} fps", q.height, q.fps)
    } else {
        format!("{}p", q.height)
    }
}

pub fn start(app: &AppHandle, sh: &Arc<Shared>, url: String, quality_id: String) -> Result<u64, String> {
    let tools = sh.tools.lock().unwrap().clone();
    if tools.ffmpeg.is_none() {
        return Err("ffmpeg is missing. It joins picture and sound; get it in Setup first.".into());
    }
    let state = app.state::<Downloads>();
    let info = state.probes.lock().unwrap().get(&url).cloned().ok_or("Look up the video again")?;
    let q = info.qualities.iter().find(|q| q.id == quality_id).cloned().ok_or("Pick a quality")?;
    let dir = sh.store.lock().unwrap().settings.folder().join("Downloads");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot write to the downloads folder: {e}"))?;

    let mut cmd = yt_dlp_base(&tools)?;
    cmd.arg("-P").arg(&dir).args(["-o", "%(title).120B [%(id)s].%(ext)s"]);
    cmd.args(["--no-playlist", "--windows-filenames", "--no-mtime", "--newline", "--progress", "--no-warnings"]);
    cmd.args(["--write-thumbnail", "--convert-thumbnails", "jpg"]);
    cmd.args(["--retries", "10", "--fragment-retries", "20", "--continue"]);
    cmd.args(["--print", "after_move:RWFILE %(filepath)s"]);
    cmd.args([
        "--progress-template",
        "download:RWP %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s %(info.format_id)s",
    ]);
    cmd.args(["-f", &selector(&q)]);
    if q.id == "audio" {
        cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "0"]);
    } else {
        cmd.args(["--merge-output-format", "mp4"]);
    }
    cmd.arg(&url).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(false);
    let mut child = cmd.spawn().map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    if let Some(handle) = child.raw_handle() {
        sh.job.adopt(handle);
    }

    let id = state.next.fetch_add(1, Ordering::Relaxed) + 1;
    state.jobs.lock().unwrap().insert(
        0,
        Job {
            id,
            url: url.clone(),
            title: info.title.clone(),
            channel: info.channel.clone(),
            thumbnail: info.thumbnail.clone(),
            quality: quality_label(&q),
            state: JobState::Downloading,
            progress: 0.0,
            speed: None,
            eta: None,
            size: q.size,
            file: None,
            error: None,
            started_at: now_ms(),
            pid: child.id(),
            video_id: info.id.clone(),
            dir: dir.clone(),
        },
    );
    emit(app);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    if let Some(stderr) = stderr {
        let tail = tail.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap();
                if t.len() >= 40 {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });
    }
    let file = Arc::new(Mutex::new(None::<String>));
    if let Some(stdout) = stdout {
        let app = app.clone();
        let file = file.clone();
        let two_parts = q.id != "audio";
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut first_format: Option<String> = None;
            let mut last_emit = 0i64;
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(path) = line.strip_prefix("RWFILE ") {
                    *file.lock().unwrap() = Some(path.trim().to_string());
                    continue;
                }
                let Some(rest) = line.strip_prefix("RWP ") else { continue };
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let num = |i: usize| parts.get(i).and_then(|s| s.parse::<f64>().ok());
                let format = parts.get(5).unwrap_or(&"").to_string();
                let done = num(0).unwrap_or(0.0);
                let total = num(1).or(num(2)).unwrap_or(0.0);
                let part = if total > 0.0 { (done / total).min(1.0) } else { 0.0 };
                first_format.get_or_insert(format.clone());
                // Picture is most of the bytes: 0-92 %, then sound 92-100 %.
                let second = first_format.as_deref() != Some(format.as_str());
                let overall = if !two_parts { part } else if second { 0.92 + part * 0.08 } else { part * 0.92 };
                let now = now_ms();
                {
                    let state = app.state::<Downloads>();
                    let mut jobs = state.jobs.lock().unwrap();
                    if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
                        job.progress = overall.max(job.progress);
                        job.speed = num(3);
                        job.eta = num(4).map(|e| e as u64);
                        if two_parts && second && part >= 1.0 {
                            job.state = JobState::Joining;
                        }
                    }
                }
                if now - last_emit > 400 {
                    last_emit = now;
                    emit(&app);
                }
            }
        });
    }

    let app = app.clone();
    let info2 = info.clone();
    tauri::async_runtime::spawn(async move {
        let status = child.wait().await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let ok = status.map(|s| s.success()).unwrap_or(false);
        let tail: String = tail.lock().unwrap().iter().cloned().collect::<Vec<_>>().join("\n");
        let path = file.lock().unwrap().clone();
        let mut toast_text = None;
        {
            let state = app.state::<Downloads>();
            let mut jobs = state.jobs.lock().unwrap();
            let Some(job) = jobs.iter_mut().find(|j| j.id == id) else { return };
            if job.state == JobState::Cancelled {
                // Already handled by cancel().
            } else if ok && path.as_deref().is_some_and(|p| Path::new(p).is_file()) {
                let path = path.unwrap();
                job.state = JobState::Done;
                job.progress = 1.0;
                job.size = std::fs::metadata(&path).ok().map(|m| m.len());
                let meta = TapeMeta {
                    channel_id: "download".into(),
                    channel_name: if info2.channel.is_empty() { "Downloads".into() } else { info2.channel.clone() },
                    platform: if info2.site.eq_ignore_ascii_case("youtube") { "youtube".into() } else { info2.site.to_ascii_lowercase() },
                    title: info2.title.clone(),
                    started_at: now_ms(),
                    ended_at: now_ms(),
                    duration_secs: info2.duration.unwrap_or(0),
                    parts: 1,
                    url: info2.url.clone(),
                    ..Default::default()
                };
                library::write_meta(&library::meta_path(Path::new(&path)), &meta);
                job.file = Some(path);
                toast_text = Some(("saved", format!("Downloaded: {}", job.title)));
            } else {
                job.state = JobState::Failed;
                job.error = Some(explain(&tail));
                toast_text = Some(("error", format!("Download failed: {}", job.error.clone().unwrap_or_default())));
            }
            job.pid = None;
        }
        if let Some((kind, text)) = toast_text {
            toast(&app, kind, text);
        }
        let _ = app.emit("tapes-changed", ());
        emit(&app);
    });
    Ok(id)
}

/// Stops a download and removes its half-downloaded files.
pub fn cancel(app: &AppHandle, id: u64) {
    let target = {
        let state = app.state::<Downloads>();
        let mut jobs = state.jobs.lock().unwrap();
        jobs.iter_mut().find(|j| j.id == id && matches!(j.state, JobState::Downloading | JobState::Joining)).map(|job| {
            job.state = JobState::Cancelled;
            (job.pid.take(), job.dir.clone(), job.video_id.clone())
        })
    };
    if let Some((pid, dir, video_id)) = target {
        if let Some(pid) = pid {
            winutil::kill_tree(pid);
        }
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            remove_partials(&dir, &video_id);
        });
    }
    emit(app);
}

fn remove_partials(dir: &Path, video_id: &str) {
    let tag = format!("[{video_id}]");
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.contains(&tag) {
            continue;
        }
        let partial = name.ends_with(".part")
            || name.ends_with(".ytdl")
            || name.contains(".part-Frag")
            || name.ends_with(".temp.mp4")
            // yt-dlp's separate picture/sound files before joining: "<title> [id].f137.mp4"
            || name.split('.').any(|p| p.len() > 1 && p.starts_with('f') && p[1..].chars().all(|c| c.is_ascii_digit()));
        if partial {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn clear_finished(app: &AppHandle) {
    app.state::<Downloads>()
        .jobs
        .lock()
        .unwrap()
        .retain(|j| matches!(j.state, JobState::Downloading | JobState::Joining));
    emit(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors() {
        let q = Quality { id: "2160p60".into(), height: 2160, fps: 60, size: None, codec: None, hdr: false };
        assert!(selector(&q).starts_with("bv*[height=2160][fps>30][dynamic_range=SDR]"));
        let q = Quality { id: "720p".into(), height: 720, fps: 30, size: None, codec: None, hdr: false };
        assert!(selector(&q).contains("[fps<=30]"));
        assert_eq!(quality_label(&q), "720p");
    }

    /// Real yt-dlp output: `yt-dlp -J <url> > $TEMP/rewind-probe.json`, then
    /// `cargo test parses_real_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn parses_real_probe() {
        let path = std::env::temp_dir().join("rewind-probe.json");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let info = parse_info("https://example", &v).unwrap();
        println!("{} | {} | {:?}s", info.title, info.channel, info.duration);
        for q in &info.qualities {
            println!("{:>10}  {:<16} {:>8} MB  {:?} hdr={}", q.id, quality_label(q), q.size.map(|s| s / 1_048_576).unwrap_or(0), q.codec, q.hdr);
        }
    }
}
