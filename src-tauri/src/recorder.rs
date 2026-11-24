//! The VCR itself: watches channels, starts yt-dlp when one goes live, keeps the tape
//! open through short drops, and turns the parts into one clean .mp4 at the end.

use crate::detect::{self, LiveInfo};
use crate::library::{self, TapeMeta};
use crate::store::{Channel, Platform, Store};
use crate::tools::Tools;
use crate::winutil::{self, Job, NO_WINDOW};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Notify;

/// A part that grew less than this is a failed start (error page, ended stream...).
const MIN_PART_BYTES: u64 = 256 * 1024;
/// No new bytes for this long = the recorder hung; it is restarted.
const STALL_MS: i64 = 180_000;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub struct Shared {
    pub store: Mutex<Store>,
    pub store_path: PathBuf,
    pub bin_dir: PathBuf,
    pub http: reqwest::Client,
    pub tools: Mutex<Tools>,
    pub status: Mutex<HashMap<String, Status>>,
    pub sessions: Mutex<HashMap<String, Session>>,
    pub job: Job,
    pub wake: Notify,
    pub quitting: AtomicBool,
    last_recording_count: AtomicUsize,
}

impl Shared {
    pub fn new(store: Store, store_path: PathBuf, bin_dir: PathBuf) -> Shared {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .expect("http client");
        Shared {
            store: Mutex::new(store),
            store_path,
            bin_dir,
            http,
            tools: Mutex::new(Tools::default()),
            status: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            job: Job::new(),
            wake: Notify::new(),
            quitting: AtomicBool::new(false),
            last_recording_count: AtomicUsize::new(usize::MAX),
        }
    }

    pub fn sessions_in(&self, states: &[SessionState]) -> usize {
        self.sessions.lock().unwrap().values().filter(|s| states.contains(&s.state)).count()
    }

    pub fn save_store(&self) -> Result<(), String> {
        let store = self.store.lock().unwrap().clone();
        store.save(&self.store_path)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(flatten)]
    pub info: LiveInfo,
    pub checked_at: i64,
    pub next_check: i64,
    pub checking: bool,
    pub error: Option<String>,
    /// The user stopped this stream: do not record it again until it goes offline.
    pub suppressed: bool,
    /// "Record now" pressed: check right away even when the channel is paused.
    pub force: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Recording,
    Holding,
    Saving,
}

pub struct Part {
    pid: u32,
    path: PathBuf,
    started_at: i64,
    bytes: u64,
    last_growth: i64,
    user_stop: bool,
}

pub struct Session {
    dir: PathBuf,
    stem: String,
    parts: Vec<PathBuf>,
    part_secs: u64,
    bytes_done: u64,
    meta: TapeMeta,
    current: Option<Part>,
    state: SessionState,
    hold_until: i64,
    thumb_refreshed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub state: SessionState,
    pub started_at: i64,
    pub part_started_at: Option<i64>,
    pub recorded_secs: u64,
    pub bytes: u64,
    pub parts: usize,
    pub hold_until: i64,
    pub title: String,
}

impl Session {
    fn view(&self) -> SessionView {
        let live_secs = self.current.as_ref().map(|p| ((now_ms() - p.started_at).max(0) / 1000) as u64).unwrap_or(0);
        SessionView {
            state: self.state,
            started_at: self.meta.started_at,
            part_started_at: self.current.as_ref().map(|p| p.started_at),
            recorded_secs: self.part_secs + live_secs,
            bytes: self.bytes_done + self.current.as_ref().map(|p| p.bytes).unwrap_or(0),
            parts: self.parts.len() + usize::from(self.current.is_some()),
            hold_until: self.hold_until,
            title: self.meta.title.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    #[serde(flatten)]
    pub channel: Channel,
    pub url: String,
    pub status: Option<Status>,
    pub session: Option<SessionView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub channels: Vec<ChannelView>,
    pub paused: bool,
    pub tools_ready: bool,
    pub folder: String,
    pub now: i64,
}

pub fn snapshot(sh: &Shared) -> Snapshot {
    let store = sh.store.lock().unwrap();
    let status = sh.status.lock().unwrap();
    let sessions = sh.sessions.lock().unwrap();
    Snapshot {
        channels: store
            .channels
            .iter()
            .map(|c| ChannelView {
                url: c.page_url(),
                status: status.get(&c.id).cloned(),
                session: sessions.get(&c.id).map(Session::view),
                channel: c.clone(),
            })
            .collect(),
        paused: store.settings.paused,
        tools_ready: sh.tools.lock().unwrap().ready(),
        folder: store.settings.folder.clone(),
        now: now_ms(),
    }
}

pub fn emit_snapshot(app: &AppHandle, sh: &Shared) {
    let snap = snapshot(sh);
    let recording = snap
        .channels
        .iter()
        .filter(|c| c.session.as_ref().is_some_and(|s| s.state == SessionState::Recording))
        .count();
    if sh.last_recording_count.swap(recording, Ordering::Relaxed) != recording {
        crate::tray::set_recording(app, recording);
    }
    let _ = app.emit("snapshot", snap);
}

pub fn toast(app: &AppHandle, kind: &str, text: impl Into<String>) {
    let _ = app.emit("toast", serde_json::json!({ "kind": kind, "text": text.into() }));
}

fn set_status(sh: &Shared, id: &str, f: impl FnOnce(&mut Status)) {
    let mut status = sh.status.lock().unwrap();
    f(status.entry(id.to_string()).or_default());
}

// ---------------------------------------------------------------- watcher loop

pub async fn run(app: AppHandle, sh: Arc<Shared>) {
    // Helpers first, then tapes left over from a crash or a power cut.
    let bin_dir = sh.bin_dir.clone();
    let tools = tauri::async_runtime::spawn_blocking(move || crate::tools::detect(&bin_dir)).await.unwrap_or_default();
    *sh.tools.lock().unwrap() = tools;
    recover_leftovers(&app, &sh).await;
    let (folder, keep) = {
        let s = &sh.store.lock().unwrap().settings;
        (s.folder(), s.keep_days)
    };
    if library::purge_older(&folder, keep) > 0 {
        let _ = app.emit("tapes-changed", ());
    }
    emit_snapshot(&app, &sh);

    let mut last_purge = now_ms();
    loop {
        if sh.quitting.load(Ordering::Relaxed) {
            break;
        }
        housekeeping(&app, &sh);
        for channel in due_channels(&sh) {
            check_channel(&app, &sh, channel).await;
        }
        emit_snapshot(&app, &sh);
        if now_ms() - last_purge > 6 * 3_600_000 {
            last_purge = now_ms();
            let (folder, keep) = {
                let s = &sh.store.lock().unwrap().settings;
                (s.folder(), s.keep_days)
            };
            if library::purge_older(&folder, keep) > 0 {
                let _ = app.emit("tapes-changed", ());
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = sh.wake.notified() => {}
        }
    }
}

fn due_channels(sh: &Shared) -> Vec<Channel> {
    let store = sh.store.lock().unwrap();
    let status = sh.status.lock().unwrap();
    let sessions = sh.sessions.lock().unwrap();
    let now = now_ms();
    store
        .channels
        .iter()
        .filter(|c| {
            let st = status.get(&c.id);
            if st.is_some_and(|s| s.checking) {
                return false;
            }
            if let Some(session) = sessions.get(&c.id) {
                if session.state != SessionState::Holding {
                    return false;
                }
            }
            let forced = st.is_some_and(|s| s.force);
            let due = st.map(|s| now >= s.next_check).unwrap_or(true);
            forced || (c.enabled && !store.settings.paused && due)
        })
        .take(4)
        .cloned()
        .collect()
}

async fn check_channel(app: &AppHandle, sh: &Arc<Shared>, channel: Channel) {
    set_status(sh, &channel.id, |s| s.checking = true);
    let result = detect::check(&sh.http, channel.platform, &channel.handle).await;
    let now = now_ms();
    let (interval, paused) = {
        let s = &sh.store.lock().unwrap().settings;
        (s.poll_seconds as i64 * 1000, s.paused)
    };
    let holding = sh
        .sessions
        .lock()
        .unwrap()
        .get(&channel.id)
        .is_some_and(|s| s.state == SessionState::Holding);

    let mut start = None;
    match result {
        Ok(info) => {
            // Twitch sends the current name/avatar with every check: keep them fresh.
            if let (Some(name), avatar) = (&info.name, &info.avatar) {
                let changed = {
                    let mut store = sh.store.lock().unwrap();
                    match store.channels.iter_mut().find(|c| c.id == channel.id) {
                        Some(c) if &c.name != name || (avatar.is_some() && &c.avatar != avatar) => {
                            c.name = name.clone();
                            if avatar.is_some() {
                                c.avatar = avatar.clone();
                            }
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    let _ = sh.save_store();
                }
            }
            let tools_ready = sh.tools.lock().unwrap().ready();
            set_status(sh, &channel.id, |s| {
                let forced = std::mem::take(&mut s.force);
                if !info.live {
                    s.suppressed = false;
                }
                s.checking = false;
                s.checked_at = now;
                s.error = None;
                s.next_check = now + if holding { 20_000 } else { interval };
                if let Some(at) = info.upcoming_at {
                    // Scheduled YouTube stream: those pages are heavier, so wait until close to the start.
                    let soon = at * 1000 - 60_000;
                    s.next_check = s.next_check.max(soon.min(now + 30 * 60_000));
                }
                if info.live && (forced || !s.suppressed) && (!paused || forced || holding) {
                    if forced {
                        s.suppressed = false;
                    }
                    if tools_ready {
                        start = Some(info.clone());
                    } else {
                        s.error = Some("Live now, but yt-dlp or ffmpeg is missing (see Settings)".into());
                    }
                }
                s.info = info;
            });
        }
        Err(err) => {
            set_status(sh, &channel.id, |s| {
                s.checking = false;
                s.force = false;
                s.checked_at = now;
                s.error = Some(err);
                s.next_check = now + interval.max(120_000);
            });
        }
    }
    if let Some(info) = start {
        if let Err(err) = start_part(app, sh, &channel, &info) {
            set_status(sh, &channel.id, |s| s.error = Some(err.clone()));
            toast(app, "error", format!("{}: {err}", channel.name));
        }
    }
}

fn quality_selector(quality: &str) -> &'static str {
    match quality {
        "1080" => "best[height<=1080]/best",
        "720" => "best[height<=720]/best",
        "480" => "best[height<=480]/worst",
        "audio" => "audio_only/bestaudio/worst",
        _ => "best",
    }
}

fn unique_stem(dir: &Path, stem: &str) -> String {
    let taken = |s: &str| {
        library::VIDEO_EXTS.iter().any(|ext| dir.join(format!("{s}.{ext}")).exists())
            || dir.join(format!("{s}.part1.ts")).exists()
    };
    if !taken(stem) {
        return stem.to_string();
    }
    (2..).map(|n| format!("{stem} ({n})")).find(|s| !taken(s)).unwrap()
}

fn start_part(app: &AppHandle, sh: &Arc<Shared>, channel: &Channel, info: &LiveInfo) -> Result<(), String> {
    let tools = sh.tools.lock().unwrap().clone();
    let (Some(yt_dlp), Some(ffmpeg)) = (tools.yt_dlp.clone(), tools.ffmpeg.clone()) else {
        return Err("yt-dlp or ffmpeg is missing".into());
    };
    let settings = sh.store.lock().unwrap().settings.clone();

    let mut sessions = sh.sessions.lock().unwrap();
    if sessions.get(&channel.id).is_some_and(|s| s.current.is_some() || s.state == SessionState::Saving) {
        return Ok(());
    }
    let is_new = !sessions.contains_key(&channel.id);
    if is_new {
        let dir = settings.folder().join(library::safe_name(&channel.name));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot write to the recordings folder: {e}"))?;
        let title = info.title.clone().unwrap_or_else(|| "Live".into());
        let stamp = chrono::Local::now().format("%Y-%m-%d %H.%M").to_string();
        let stem = unique_stem(&dir, &format!("{stamp} - {}", library::safe_name(&title)));
        let url = match (channel.platform, &info.video_id) {
            (Platform::Youtube, Some(id)) => format!("https://www.youtube.com/watch?v={id}"),
            _ => channel.page_url(),
        };
        let meta = TapeMeta {
            channel_id: channel.id.clone(),
            channel_name: channel.name.clone(),
            platform: channel.platform.label().into(),
            title,
            game: info.game.clone(),
            started_at: now_ms(),
            url,
            ..Default::default()
        };
        // Written now so a crash or power cut still leaves the tape's details.
        library::write_meta(&dir.join(format!("{stem}.json")), &meta);
        sessions.insert(
            channel.id.clone(),
            Session {
                dir,
                stem,
                parts: Vec::new(),
                part_secs: 0,
                bytes_done: 0,
                meta,
                current: None,
                state: SessionState::Recording,
                hold_until: 0,
                thumb_refreshed: false,
            },
        );
    }
    let session = sessions.get_mut(&channel.id).unwrap();
    let index = session.parts.len() + 1;
    let path = session.dir.join(format!("{}.part{index}.ts", session.stem));
    let url = match (channel.platform, &info.video_id) {
        (Platform::Youtube, Some(id)) => format!("https://www.youtube.com/watch?v={id}"),
        (Platform::Youtube, None) => format!("{}/live", channel.page_url()),
        (Platform::Twitch, _) => channel.page_url(),
    };

    let mut cmd = tokio::process::Command::new(&yt_dlp);
    cmd.arg(&url)
        .args(["-f", quality_selector(&settings.quality)])
        .arg("-o")
        .arg(path.to_string_lossy().replace('%', "%%"))
        .args(["--no-part", "--hls-use-mpegts", "--no-mtime", "--no-progress", "--no-playlist"])
        .args(["--retries", "10", "--fragment-retries", "30"])
        .arg("--ffmpeg-location")
        .arg(&ffmpeg);
    if channel.platform == Platform::Youtube {
        if let Some(node) = &tools.node {
            cmd.arg("--js-runtimes").arg(format!("node:{}", node.display()));
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(NO_WINDOW)
        .kill_on_drop(false);
    let mut child = cmd.spawn().map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    if let Some(handle) = child.raw_handle() {
        sh.job.adopt(handle);
    }
    let pid = child.id().unwrap_or(0);
    let now = now_ms();
    session.current = Some(Part { pid, path, started_at: now, bytes: 0, last_growth: now, user_stop: false });
    session.state = SessionState::Recording;
    session.hold_until = 0;
    let thumb_target = session.dir.join(format!("{}.jpg", session.stem));
    drop(sessions);

    if let Some(url) = info.thumbnail.clone() {
        if is_new {
            spawn_thumb(sh.clone(), url, thumb_target);
        }
    }
    set_status(sh, &channel.id, |s| s.error = None);
    if is_new {
        toast(app, "rec", format!("Recording {}", channel.name));
    } else {
        toast(app, "rec", format!("{} is back. Same tape continues.", channel.name));
    }

    let stderr = child.stderr.take();
    let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    if let Some(stderr) = stderr {
        let tail = tail.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap();
                if t.len() >= 30 {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });
    }
    let app = app.clone();
    let sh = sh.clone();
    let id = channel.id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        // Let the stderr reader drain the last lines.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let tail: Vec<String> = tail.lock().unwrap().iter().cloned().collect();
        on_part_exit(&app, &sh, &id, &tail);
    });
    Ok(())
}

fn spawn_thumb(sh: Arc<Shared>, url: String, target: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let sep = if url.contains('?') { '&' } else { '?' };
        let url = format!("{url}{sep}t={}", now_ms());
        if let Ok(resp) = sh.http.get(&url).send().await.and_then(|r| r.error_for_status()) {
            if let Ok(bytes) = resp.bytes().await {
                if bytes.len() > 2048 {
                    let _ = tokio::fs::write(&target, &bytes).await;
                }
            }
        }
    });
}

/// Turns yt-dlp's last lines into one short sentence for the UI.
fn explain(tail: &[String]) -> String {
    let error = tail.iter().rev().find(|l| l.contains("ERROR")).or_else(|| tail.last());
    let Some(line) = error else {
        return "The recorder stopped right away".into();
    };
    let lower = line.to_ascii_lowercase();
    if lower.contains("not currently live") || lower.contains("offline") {
        "The stream ended before recording started".into()
    } else if lower.contains("members-only") || lower.contains("join this channel") {
        "This stream is for channel members only".into()
    } else if lower.contains("sign in") || lower.contains("age") && lower.contains("confirm") {
        "YouTube wants a signed-in viewer for this stream (age or region limit)".into()
    } else if lower.contains("subscriber") {
        "This stream is for subscribers only".into()
    } else {
        let text = line.replace("ERROR: ", "");
        text.chars().take(160).collect()
    }
}

fn on_part_exit(app: &AppHandle, sh: &Arc<Shared>, id: &str, tail: &[String]) {
    let hold_minutes = sh.store.lock().unwrap().settings.hold_minutes;
    let quitting = sh.quitting.load(Ordering::Relaxed);
    let now = now_ms();
    let mut finalize_now = false;
    let mut failed_empty = None;
    // Status changes are applied after the sessions lock is released
    // (lock order everywhere: store -> status -> sessions).
    let mut status_error: Option<String> = None;
    let mut next_check: Option<i64> = None;
    {
        let mut sessions = sh.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(id) else {
            return;
        };
        let Some(part) = session.current.take() else {
            return;
        };
        let size = std::fs::metadata(&part.path).map(|m| m.len()).unwrap_or(0);
        if size < MIN_PART_BYTES {
            let _ = std::fs::remove_file(&part.path);
            let reason = explain(tail);
            status_error = Some(reason.clone());
            next_check = Some(now + 180_000);
            if session.parts.is_empty() {
                let meta = session.dir.join(format!("{}.json", session.stem));
                let thumb = session.dir.join(format!("{}.jpg", session.stem));
                sessions.remove(id);
                let _ = std::fs::remove_file(meta);
                let _ = std::fs::remove_file(thumb);
                failed_empty = Some(reason);
            } else if part.user_stop || quitting {
                finalize_now = true;
            } else {
                session.state = SessionState::Holding;
                if session.hold_until == 0 {
                    session.hold_until = now + hold_minutes as i64 * 60_000;
                }
            }
        } else {
            session.parts.push(part.path.clone());
            session.part_secs += ((now - part.started_at).max(0) / 1000) as u64;
            session.bytes_done += size;
            if part.user_stop || quitting || hold_minutes == 0 {
                finalize_now = true;
            } else {
                session.state = SessionState::Holding;
                session.hold_until = now + hold_minutes as i64 * 60_000;
                next_check = Some(now + 20_000);
            }
        }
        if finalize_now {
            if let Some(s) = sessions.get_mut(id) {
                s.state = SessionState::Saving;
            }
        }
    }
    if status_error.is_some() || next_check.is_some() {
        set_status(sh, id, |s| {
            if let Some(err) = status_error {
                s.error = Some(err);
            }
            if let Some(at) = next_check {
                s.next_check = at;
            }
        });
    }
    if let Some(reason) = failed_empty {
        toast(app, "error", reason);
    }
    if finalize_now {
        spawn_finalize(app.clone(), sh.clone(), id.to_string());
    }
    sh.wake.notify_one();
}

/// Sizes, hung recorders, cover refresh, and tapes whose hold time ran out.
fn housekeeping(app: &AppHandle, sh: &Arc<Shared>) {
    let now = now_ms();
    let mut stalled = Vec::new();
    let mut to_finalize = Vec::new();
    let mut thumbs = Vec::new();
    {
        let mut sessions = sh.sessions.lock().unwrap();
        for (id, session) in sessions.iter_mut() {
            if let Some(part) = session.current.as_mut() {
                let size = std::fs::metadata(&part.path).map(|m| m.len()).unwrap_or(0);
                if size > part.bytes {
                    part.bytes = size;
                    part.last_growth = now;
                } else if now - part.last_growth > STALL_MS {
                    part.last_growth = now;
                    stalled.push(part.pid);
                }
                if !session.thumb_refreshed && now - session.meta.started_at > 10 * 60_000 {
                    // The first cover is often a "starting soon" screen: take a new one 10 minutes in.
                    session.thumb_refreshed = true;
                    thumbs.push((id.clone(), session.dir.join(format!("{}.jpg", session.stem))));
                }
            } else if session.state == SessionState::Holding && now >= session.hold_until {
                session.state = SessionState::Saving;
                to_finalize.push(id.clone());
            }
        }
    }
    for pid in stalled {
        winutil::kill_tree(pid);
    }
    for id in to_finalize {
        spawn_finalize(app.clone(), sh.clone(), id);
    }
    for (id, target) in thumbs {
        let url = sh.status.lock().unwrap().get(&id).and_then(|s| s.info.thumbnail.clone());
        if let Some(url) = url {
            spawn_thumb(sh.clone(), url, target);
        }
    }
}

fn spawn_finalize(app: AppHandle, sh: Arc<Shared>, id: String) {
    tauri::async_runtime::spawn(async move {
        let job = {
            let sessions = sh.sessions.lock().unwrap();
            sessions.get(&id).map(|s| (s.dir.clone(), s.stem.clone(), s.parts.clone(), s.meta.clone(), s.part_secs))
        };
        let Some((dir, stem, parts, mut meta, secs)) = job else {
            return;
        };
        meta.ended_at = now_ms();
        meta.duration_secs = secs;
        meta.parts = parts.len() as u32;
        let ffmpeg = sh.tools.lock().unwrap().ffmpeg.clone();
        let saved = tauri::async_runtime::spawn_blocking(move || finalize_files(ffmpeg.as_deref(), &dir, &stem, &parts, &meta))
            .await
            .unwrap_or(None);
        sh.sessions.lock().unwrap().remove(&id);
        set_status(&sh, &id, |s| s.next_check = now_ms() + 5_000);
        if let Some(name) = saved {
            toast(&app, "saved", format!("Saved tape: {name}"));
        }
        let _ = app.emit("tapes-changed", ());
        let (folder, keep) = {
            let s = &sh.store.lock().unwrap().settings;
            (s.folder(), s.keep_days)
        };
        library::purge_older(&folder, keep);
        emit_snapshot(&app, &sh);
        sh.wake.notify_one();
    });
}

/// Parts (.ts) -> one `<stem>.mp4` with ffmpeg (stream copy: fast, no quality loss).
/// Without ffmpeg, or if it fails, the parts are renamed to plain .ts tapes instead.
/// Returns the saved file name.
pub fn finalize_files(ffmpeg: Option<&Path>, dir: &Path, stem: &str, parts: &[PathBuf], meta: &TapeMeta) -> Option<String> {
    if parts.is_empty() {
        let _ = std::fs::remove_file(dir.join(format!("{stem}.json")));
        let _ = std::fs::remove_file(dir.join(format!("{stem}.jpg")));
        return None;
    }
    let out = dir.join(format!("{stem}.mp4"));
    let list = dir.join(format!("{stem}.concat.txt"));
    let mut ok = false;
    if let Some(ffmpeg) = ffmpeg {
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        if parts.len() == 1 {
            cmd.arg("-i").arg(&parts[0]);
        } else {
            let text: String = parts
                .iter()
                .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''")))
                .collect();
            if std::fs::write(&list, text).is_ok() {
                cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&list);
            }
        }
        cmd.args(["-map", "0:v?", "-map", "0:a?", "-c", "copy", "-movflags", "+faststart"]).arg(&out);
        ok = cmd
            .creation_flags(NO_WINDOW)
            .stdin(Stdio::null())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
            && std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false);
        let _ = std::fs::remove_file(&list);
    }
    if ok {
        for part in parts {
            let _ = std::fs::remove_file(part);
        }
        library::write_meta(&dir.join(format!("{stem}.json")), meta);
        return out.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    let _ = std::fs::remove_file(&out);
    // Fallback: keep the raw recording, playable in VLC / the default player.
    if parts.len() == 1 {
        let ts = dir.join(format!("{stem}.ts"));
        if std::fs::rename(&parts[0], &ts).is_ok() {
            library::write_meta(&dir.join(format!("{stem}.json")), meta);
        }
        return ts.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    for (i, part) in parts.iter().enumerate() {
        let name = format!("{stem} (part {}).ts", i + 1);
        let target = dir.join(&name);
        if std::fs::rename(part, &target).is_ok() {
            let mut m = meta.clone();
            m.title = format!("{} (part {})", meta.title, i + 1);
            library::write_meta(&target.with_extension("json"), &m);
        }
    }
    Some(format!("{stem} ({} parts)", parts.len()))
}

async fn recover_leftovers(app: &AppHandle, sh: &Arc<Shared>) {
    let folder = sh.store.lock().unwrap().settings.folder();
    let groups = library::leftovers(&folder);
    if groups.is_empty() {
        return;
    }
    let ffmpeg = sh.tools.lock().unwrap().ffmpeg.clone();
    let count = groups.len();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        for (dir, stem, parts) in groups {
            let meta_file = dir.join(format!("{stem}.json"));
            let mut meta = library::read_meta(&meta_file).unwrap_or_default();
            let last = parts.last().map(|p| library::file_modified_ms(p)).unwrap_or(0);
            if meta.started_at == 0 {
                meta.started_at = parts.first().map(|p| library::file_modified_ms(p)).unwrap_or(last);
            }
            meta.ended_at = last;
            meta.duration_secs = ((last - meta.started_at).max(0) / 1000) as u64;
            meta.parts = parts.len() as u32;
            finalize_files(ffmpeg.as_deref(), &dir, &stem, &parts, &meta);
        }
    })
    .await;
    toast(app, "saved", format!("Rescued {count} unfinished tape(s) from last time"));
    let _ = app.emit("tapes-changed", ());
}

// ---------------------------------------------------------------- user actions

pub fn stop(app: &AppHandle, sh: &Arc<Shared>, id: &str) {
    let mut pid = None;
    let mut finalize = false;
    {
        let mut sessions = sh.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(id) {
            if let Some(part) = session.current.as_mut() {
                part.user_stop = true;
                pid = Some(part.pid);
            } else if session.state == SessionState::Holding {
                session.state = SessionState::Saving;
                finalize = true;
            }
        }
    }
    set_status(sh, id, |s| s.suppressed = true);
    if let Some(pid) = pid {
        winutil::kill_tree(pid);
    }
    if finalize {
        spawn_finalize(app.clone(), sh.clone(), id.to_string());
    }
    emit_snapshot(app, sh);
}

pub fn record_now(sh: &Arc<Shared>, id: &str) {
    set_status(sh, id, |s| {
        s.suppressed = false;
        s.force = true;
        s.next_check = 0;
    });
    sh.wake.notify_one();
}

pub fn check_now(sh: &Arc<Shared>, id: Option<&str>) {
    {
        let mut status = sh.status.lock().unwrap();
        match id {
            Some(id) => status.entry(id.to_string()).or_default().next_check = 0,
            None => status.values_mut().for_each(|s| s.next_check = 0),
        }
    }
    sh.wake.notify_one();
}

pub fn forget(sh: &Arc<Shared>, id: &str) {
    sh.status.lock().unwrap().remove(id);
}

pub fn active_count(sh: &Shared) -> usize {
    sh.sessions.lock().unwrap().len()
}

/// Quit: stop every recorder, save every open tape, then exit.
pub async fn shutdown(app: AppHandle, sh: Arc<Shared>) {
    if sh.quitting.load(Ordering::Relaxed) {
        return;
    }
    save_all(&app, &sh).await;
    app.exit(0);
}

/// Stops the watcher and every recorder and saves all open tapes (used by quit and updates).
pub async fn save_all(app: &AppHandle, sh: &Arc<Shared>) {
    let app = app.clone();
    let sh = sh.clone();
    if sh.quitting.swap(true, Ordering::Relaxed) {
        return;
    }
    sh.wake.notify_one();
    let (pids, holding): (Vec<u32>, Vec<String>) = {
        let mut sessions = sh.sessions.lock().unwrap();
        let mut pids = Vec::new();
        let mut holding = Vec::new();
        for (id, s) in sessions.iter_mut() {
            if let Some(part) = s.current.as_mut() {
                part.user_stop = true;
                pids.push(part.pid);
            } else if s.state == SessionState::Holding {
                s.state = SessionState::Saving;
                holding.push(id.clone());
            }
        }
        (pids, holding)
    };
    if !pids.is_empty() || !holding.is_empty() {
        crate::tray::set_tooltip(&app, "Rewind - saving tapes...");
        toast(&app, "saved", "Saving your tapes before closing...");
    }
    for pid in pids {
        winutil::kill_tree(pid);
    }
    for id in holding {
        spawn_finalize(app.clone(), sh.clone(), id);
    }
    // Remuxing a long stream can take a little while on a slow disk.
    for _ in 0..1200 {
        if active_count(&sh) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub fn store_of(app: &AppHandle) -> Arc<Shared> {
    app.state::<Arc<Shared>>().inner().clone()
}
