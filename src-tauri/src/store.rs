//! Settings + channel list, saved as one JSON file in the app config folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Twitch,
    Youtube,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Twitch => "twitch",
            Platform::Youtube => "youtube",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    /// "twitch:login" or "youtube:@handle" / "youtube:channel/UC...".
    pub id: String,
    pub platform: Platform,
    /// Twitch login, or the YouTube path after youtube.com/ ("@handle", "channel/UC...").
    pub handle: String,
    pub name: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Index of the cassette label colour in the UI.
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub added_at: i64,
}

fn yes() -> bool {
    true
}

impl Channel {
    pub fn page_url(&self) -> String {
        match self.platform {
            Platform::Twitch => format!("https://www.twitch.tv/{}", self.handle),
            Platform::Youtube => format!("https://www.youtube.com/{}", self.handle),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub folder: String,
    /// best | 1080 | 720 | 480 | audio
    pub quality: String,
    pub poll_seconds: u64,
    /// How long a tape stays open after a stream drops, so a quick reconnect
    /// continues the same recording instead of starting a new file.
    pub hold_minutes: u64,
    pub autostart: bool,
    pub start_hidden: bool,
    pub close_to_tray: bool,
    /// 0 = keep forever.
    pub keep_days: u64,
    pub sounds: bool,
    pub paused: bool,
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            folder: String::new(),
            quality: "best".into(),
            poll_seconds: 90,
            hold_minutes: 10,
            autostart: false,
            start_hidden: true,
            close_to_tray: true,
            keep_days: 0,
            sounds: true,
            paused: false,
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self, default_folder: &Path) {
        if self.folder.trim().is_empty() {
            self.folder = default_folder.to_string_lossy().into_owned();
        }
        self.poll_seconds = self.poll_seconds.clamp(30, 1800);
        self.hold_minutes = self.hold_minutes.min(120);
        if !["best", "1080", "720", "480", "audio"].contains(&self.quality.as_str()) {
            self.quality = "best".into();
        }
    }

    pub fn folder(&self) -> PathBuf {
        PathBuf::from(&self.folder)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub settings: Settings,
    pub channels: Vec<Channel>,
}

impl Store {
    pub fn load(path: &Path, default_folder: &Path) -> Store {
        let mut store: Store = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        store.settings.sanitize(default_folder);
        store
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}
