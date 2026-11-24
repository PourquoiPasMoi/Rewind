//! "Is this channel live?" — done in Rust with tiny requests, no yt-dlp process.
//!
//! Twitch: one GraphQL call (~1 KB).
//! YouTube: the mobile `/live` page. Its canonical link sits in the first few KB:
//! a channel link means "not live" and we stop reading right there. Only when it
//! points at a video do we read on to `isLiveNow` (live or scheduled).

use crate::store::Platform;
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

const TWITCH_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
const TWITCH_QUERY: &str = "query($l:String!){user(login:$l){login displayName profileImageURL(width:300) stream{id title type createdAt viewersCount game{displayName} previewImageURL(width:640,height:360)}}}";
const MOBILE_UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36";
const YT_COOKIE: &str = "SOCS=CAI; CONSENT=YES+";

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveInfo {
    pub live: bool,
    pub title: Option<String>,
    pub game: Option<String>,
    pub viewers: Option<u64>,
    pub started_at: Option<String>,
    pub thumbnail: Option<String>,
    pub video_id: Option<String>,
    /// Unix seconds of a scheduled (not yet started) YouTube stream.
    pub upcoming_at: Option<i64>,
    pub name: Option<String>,
    pub avatar: Option<String>,
}

pub struct Resolved {
    pub platform: Platform,
    pub handle: String,
    pub name: String,
    pub avatar: Option<String>,
}

fn net(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "The site took too long to answer".into()
    } else if e.is_connect() {
        "No internet connection".into()
    } else {
        format!("Network error: {e}")
    }
}

/// Turns whatever the user pasted into (platform, handle).
pub fn parse_input(input: &str) -> Result<(Platform, String), String> {
    let raw = input.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("Paste a Twitch or YouTube channel link".into());
    }
    let lower = raw.to_ascii_lowercase();
    let path_after = |marker: &str| -> Option<String> {
        let at = lower.find(marker)? + marker.len();
        let rest = &raw[at..];
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        Some(rest.trim_matches('/').to_string())
    };

    if let Some(rest) = path_after("twitch.tv/") {
        let login = rest.split('/').next().unwrap_or("").to_ascii_lowercase();
        if valid_twitch(&login) {
            return Ok((Platform::Twitch, login));
        }
        return Err("That Twitch link has no channel name in it".into());
    }
    if lower.contains("youtu.be/") || lower.contains("watch?v=") || lower.contains("/shorts/") {
        return Err("That is a video link. Paste the channel link instead (youtube.com/@name)".into());
    }
    if let Some(rest) = path_after("youtube.com/") {
        let mut parts = rest.split('/');
        let first = parts.next().unwrap_or("");
        if first.starts_with('@') && first.len() > 1 {
            return Ok((Platform::Youtube, first.to_string()));
        }
        if matches!(first, "channel" | "c" | "user") {
            if let Some(second) = parts.next().filter(|s| !s.is_empty()) {
                return Ok((Platform::Youtube, format!("{first}/{second}")));
            }
        }
        return Err("Paste a YouTube channel link like youtube.com/@name".into());
    }
    if raw.starts_with('@') && raw.len() > 1 && !raw.contains(' ') {
        return Ok((Platform::Youtube, raw.to_string()));
    }
    let login = raw.to_ascii_lowercase();
    if valid_twitch(&login) {
        return Ok((Platform::Twitch, login));
    }
    Err("Paste a Twitch or YouTube channel link".into())
}

fn valid_twitch(login: &str) -> bool {
    (2..=25).contains(&login.len()) && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub async fn resolve(http: &reqwest::Client, platform: Platform, handle: &str) -> Result<Resolved, String> {
    match platform {
        Platform::Twitch => {
            let info = twitch(http, handle).await?.ok_or("No Twitch channel with that name")?;
            Ok(Resolved {
                platform,
                handle: handle.to_string(),
                name: info.name.unwrap_or_else(|| handle.to_string()),
                avatar: info.avatar,
            })
        }
        Platform::Youtube => youtube_resolve(http, handle).await,
    }
}

pub async fn check(http: &reqwest::Client, platform: Platform, handle: &str) -> Result<LiveInfo, String> {
    match platform {
        Platform::Twitch => twitch(http, handle).await?.ok_or_else(|| "This Twitch channel no longer exists".into()),
        Platform::Youtube => youtube_live(http, handle).await,
    }
}

async fn twitch(http: &reqwest::Client, login: &str) -> Result<Option<LiveInfo>, String> {
    let body = serde_json::json!({ "query": TWITCH_QUERY, "variables": { "l": login } });
    let value: serde_json::Value = http
        .post("https://gql.twitch.tv/gql")
        .header("Client-Id", TWITCH_CLIENT_ID)
        .json(&body)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?
        .json()
        .await
        .map_err(net)?;
    let user = &value["data"]["user"];
    if user.is_null() {
        return Ok(None);
    }
    let text = |v: &serde_json::Value| v.as_str().map(str::to_string);
    let mut info = LiveInfo {
        name: text(&user["displayName"]),
        avatar: text(&user["profileImageURL"]),
        ..Default::default()
    };
    let stream = &user["stream"];
    if !stream.is_null() {
        info.live = true;
        info.title = text(&stream["title"]);
        info.game = text(&stream["game"]["displayName"]);
        info.viewers = stream["viewersCount"].as_u64();
        info.started_at = text(&stream["createdAt"]);
        info.thumbnail = text(&stream["previewImageURL"]);
    }
    Ok(Some(info))
}

/// Reads a response body chunk by chunk until `done` says it has enough (or `max` bytes).
/// Dropping the response early closes the connection, so the rest is never downloaded.
async fn read_until(mut resp: reqwest::Response, max: usize, done: impl Fn(&str) -> bool) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    while let Some(chunk) = resp.chunk().await.map_err(net)? {
        buf.extend_from_slice(&chunk);
        if buf.len() >= max || done(&String::from_utf8_lossy(&buf)) {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn yt_get(http: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    http.get(url)
        .header("User-Agent", MOBILE_UA)
        .header("Accept-Language", "en-US,en;q=0.8")
        .header("Cookie", YT_COOKIE)
}

static CANONICAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"rel="canonical" href="([^"]+)""#).unwrap());
static VIDEO_DETAILS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""videoDetails":\{"videoId":"([^"]+)","title":"((?:[^"\\]|\\.)*)""#).unwrap());
static META_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<meta name="title" content="([^"]*)""#).unwrap());
static OG_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"property="og:title" content="([^"]*)""#).unwrap());
static OG_IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"property="og:image" content="([^"]*)""#).unwrap());
static START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""startTimestamp":"([^"]+)""#).unwrap());
static SCHEDULED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""scheduledStartTime":"(\d+)""#).unwrap());
static ANY_VIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""videoId":"([A-Za-z0-9_-]{11})""#).unwrap());

fn capture(re: &Regex, text: &str) -> Option<String> {
    re.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn json_unescape(s: &str) -> String {
    serde_json::from_str::<String>(&format!("\"{s}\"")).unwrap_or_else(|_| s.to_string())
}

async fn youtube_live(http: &reqwest::Client, path: &str) -> Result<LiveInfo, String> {
    let url = format!("https://m.youtube.com/{path}/live");
    let resp = yt_get(http, &url).send().await.map_err(net)?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("This YouTube channel was not found".into());
    }
    let resp = resp.error_for_status().map_err(net)?;
    let text = read_until(resp, 4 * 1024 * 1024, |s| match capture(&CANONICAL, s) {
        Some(c) if !c.contains("watch?v=") => true,
        Some(_) => s.contains("\"isLiveNow\":") && s.contains("\"videoDetails\":{"),
        None => false,
    })
    .await?;

    let canonical = capture(&CANONICAL, &text);
    let video_id = match &canonical {
        Some(c) if c.contains("watch?v=") => c.split("watch?v=").nth(1).map(|v| v.chars().take(11).collect::<String>()),
        Some(_) => return Ok(LiveInfo::default()),
        None if text.contains("\"isLiveNow\":true") => capture(&ANY_VIDEO, &text),
        None => return Ok(LiveInfo::default()),
    };
    let Some(video_id) = video_id else {
        return Ok(LiveInfo::default());
    };

    let mut info = LiveInfo {
        live: text.contains("\"isLiveNow\":true"),
        title: VIDEO_DETAILS
            .captures(&text)
            .and_then(|c| c.get(2))
            .map(|m| json_unescape(m.as_str()))
            .or_else(|| capture(&META_TITLE, &text).map(|t| html_unescape(&t))),
        started_at: capture(&START, &text),
        thumbnail: Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault_live.jpg")),
        video_id: Some(video_id),
        ..Default::default()
    };
    if !info.live {
        info.upcoming_at = capture(&SCHEDULED, &text).and_then(|s| s.parse().ok());
    }
    Ok(info)
}

async fn youtube_resolve(http: &reqwest::Client, path: &str) -> Result<Resolved, String> {
    let url = format!("https://m.youtube.com/{path}");
    let resp = yt_get(http, &url).send().await.map_err(net)?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("No YouTube channel with that name".into());
    }
    let resp = resp.error_for_status().map_err(net)?;
    let text = read_until(resp, 2 * 1024 * 1024, |s| s.contains("property=\"og:image\"") && s.contains("property=\"og:title\"")).await?;
    let name = capture(&OG_TITLE, &text).map(|t| html_unescape(&t)).ok_or("No YouTube channel with that name")?;
    Ok(Resolved {
        platform: Platform::Youtube,
        handle: path.to_string(),
        name,
        avatar: capture(&OG_IMAGE, &text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        assert_eq!(parse_input("https://www.twitch.tv/Shroud?x=1").unwrap(), (Platform::Twitch, "shroud".into()));
        assert_eq!(parse_input("shroud").unwrap(), (Platform::Twitch, "shroud".into()));
        assert_eq!(parse_input("https://youtube.com/@LofiGirl/live").unwrap(), (Platform::Youtube, "@LofiGirl".into()));
        assert_eq!(parse_input("@LofiGirl").unwrap(), (Platform::Youtube, "@LofiGirl".into()));
        assert_eq!(
            parse_input("https://www.youtube.com/channel/UCSJ4gkVC6NrvII8umztf0Ow").unwrap(),
            (Platform::Youtube, "channel/UCSJ4gkVC6NrvII8umztf0Ow".into())
        );
        assert!(parse_input("https://www.youtube.com/watch?v=abc").is_err());
        assert!(parse_input("").is_err());
    }
}
