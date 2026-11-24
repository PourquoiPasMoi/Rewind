//! The tape shelf: finished recordings in the save folder, one sub-folder per channel.
//! Each video `<stem>.mp4` can have `<stem>.json` (details) and `<stem>.jpg` (cover).

use crate::winutil;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const VIDEO_EXTS: [&str; 5] = ["mp4", "mkv", "ts", "m4a", "webm"];

static PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(.*)\.part(\d+)\.ts$").unwrap());

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TapeMeta {
    pub channel_id: String,
    pub channel_name: String,
    pub platform: String,
    pub title: String,
    pub game: Option<String>,
    /// Unix milliseconds.
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_secs: u64,
    pub parts: u32,
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tape {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    /// WebView2 plays mp4 / m4a / webm; .ts and .mkv open in the default player.
    pub playable: bool,
    pub thumb: Option<String>,
    #[serde(flatten)]
    pub meta: TapeMeta,
}

/// Safe file/folder name: no Windows-reserved characters, no `%` (yt-dlp templates), short.
pub fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*%"#.contains(c) { ' ' } else { c })
        .collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > 80 {
        out = out.chars().take(80).collect::<String>().trim_end().to_string();
    }
    let out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        "Live".into()
    } else {
        out
    }
}

pub fn meta_path(video: &Path) -> PathBuf {
    video.with_extension("json")
}

pub fn thumb_path(video: &Path) -> PathBuf {
    video.with_extension("jpg")
}

pub fn read_meta(path: &Path) -> Option<TapeMeta> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn write_meta(path: &Path, meta: &TapeMeta) {
    if let Ok(text) = serde_json::to_string_pretty(meta) {
        let _ = std::fs::write(path, text);
    }
}

fn modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Video files in the folder and its direct sub-folders.
fn videos(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(top) = std::fs::read_dir(folder) else {
        return found;
    };
    let mut push = |path: PathBuf| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if VIDEO_EXTS.contains(&ext.as_str()) && !PART.is_match(&name) {
            found.push(path);
        }
    };
    for entry in top.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                for file in inner.flatten() {
                    if file.path().is_file() {
                        push(file.path());
                    }
                }
            }
        } else {
            push(path);
        }
    }
    found
}

pub fn list(folder: &Path) -> Vec<Tape> {
    let mut tapes: Vec<Tape> = videos(folder)
        .into_iter()
        .map(|video| {
            let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut meta = read_meta(&meta_path(&video)).unwrap_or_default();
            if meta.title.is_empty() {
                meta.title = stem.split_once(" - ").map(|(_, t)| t.to_string()).unwrap_or_else(|| stem.clone());
            }
            if meta.channel_name.is_empty() {
                let parent = video.parent().unwrap_or(folder);
                if parent != folder {
                    meta.channel_name = parent.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                }
            }
            if meta.started_at == 0 {
                meta.started_at = modified_ms(&video);
            }
            let ext = video.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let thumb = thumb_path(&video);
            Tape {
                size: std::fs::metadata(&video).map(|m| m.len()).unwrap_or(0),
                playable: matches!(ext.as_str(), "mp4" | "m4a" | "webm"),
                thumb: thumb.is_file().then(|| thumb.to_string_lossy().into_owned()),
                file_name: video.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: video.to_string_lossy().into_owned(),
                meta,
            }
        })
        .collect();
    tapes.sort_by(|a, b| b.meta.started_at.cmp(&a.meta.started_at));
    tapes
}

/// Only files inside the save folder may be touched from the UI.
pub fn inside(folder: &Path, path: &Path) -> bool {
    let (Ok(folder), Ok(path)) = (folder.canonicalize(), path.canonicalize()) else {
        return false;
    };
    path.starts_with(folder)
}

/// Recycle bin: video + details + cover.
pub fn delete(folder: &Path, video: &Path) -> Result<(), String> {
    if !inside(folder, video) {
        return Err("That file is not in the recordings folder".into());
    }
    let meta = meta_path(video);
    let thumb = thumb_path(video);
    winutil::recycle(&[video, &meta, &thumb])
}

/// Permanently removes tapes older than `days` (the user turned this on in Settings).
pub fn purge_older(folder: &Path, days: u64) -> usize {
    if days == 0 {
        return 0;
    }
    let cutoff = chrono::Utc::now().timestamp_millis() - (days as i64) * 86_400_000;
    let mut removed = 0;
    for tape in list(folder) {
        if tape.meta.started_at > 0 && tape.meta.started_at < cutoff {
            let video = PathBuf::from(&tape.path);
            if std::fs::remove_file(&video).is_ok() {
                let _ = std::fs::remove_file(meta_path(&video));
                let _ = std::fs::remove_file(thumb_path(&video));
                removed += 1;
            }
        }
    }
    removed
}

/// `.partN.ts` files left behind (app closed or PC turned off while recording),
/// grouped by tape: (folder, stem, parts in order).
pub fn leftovers(folder: &Path) -> Vec<(PathBuf, String, Vec<PathBuf>)> {
    let mut groups: std::collections::BTreeMap<(PathBuf, String), Vec<(u32, PathBuf)>> = Default::default();
    let mut scan = |dir: &Path| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(c) = PART.captures(&name) {
                    let stem = c[1].to_string();
                    let n: u32 = c[2].parse().unwrap_or(1);
                    groups.entry((dir.to_path_buf(), stem)).or_default().push((n, entry.path()));
                }
            }
        }
    };
    scan(folder);
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                scan(&entry.path());
            }
        }
    }
    groups
        .into_iter()
        .map(|((dir, stem), mut parts)| {
            parts.sort_by_key(|(n, _)| *n);
            (dir, stem, parts.into_iter().map(|(_, p)| p).collect())
        })
        .collect()
}

pub fn file_modified_ms(path: &Path) -> i64 {
    modified_ms(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("a/b:c*d?  100%"), "a b c d 100");
        assert_eq!(safe_name("..."), "Live");
        assert_eq!(safe_name("🎄 cozy"), "🎄 cozy");
    }
}
