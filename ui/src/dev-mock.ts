// Preview only (ui/preview.html): fakes the Rust side so screens can be checked in a
// normal browser while the real Rewind is running. Never loaded by the app itself.

type Args = Record<string, unknown>;
const now = Date.now();
const day = (d: Date) => (d.getDay() + 6) % 7; // Monday = 0
const today = new Date();
const minutesNow = today.getHours() * 60 + today.getMinutes();

const channels = [
  {
    id: "twitch:doigby", platform: "twitch", handle: "doigby", name: "Doigby", avatar: null, enabled: true, color: 0, addedAt: now,
    url: "https://www.twitch.tv/doigby",
    timers: [{ days: 0b0010101, start: 21 * 60, end: 60, enabled: true }],
    scheduled: true, timerActive: false, nextTimer: now + 5 * 3600_000,
    status: { live: false, checkedAt: now - 3600_000, nextCheck: 0, checking: false, suppressed: false, force: false },
    session: null,
  },
  {
    id: "twitch:kaicenat", platform: "twitch", handle: "kaicenat", name: "KaiCenat", avatar: null, enabled: true, color: 2, addedAt: now,
    url: "https://www.twitch.tv/kaicenat",
    timers: [{ days: 1 << day(today), start: (minutesNow + 1380) % 1440, end: (minutesNow + 180) % 1440, enabled: true }],
    scheduled: true, timerActive: true, nextTimer: now + 86400_000,
    status: { live: true, title: "First to a million in GTA 6", viewers: 29890, checkedAt: now - 20_000, nextCheck: now + 10_000, checking: false, suppressed: false, force: false },
    session: { state: "recording", startedAt: now - 3_725_000, partStartedAt: now - 3_725_000, recordedSecs: 3725, bytes: 2.1 * 1024 ** 3, parts: 1, holdUntil: 0, title: "First to a million in GTA 6" },
  },
  {
    id: "youtube:@lofigirl", platform: "youtube", handle: "@LofiGirl", name: "Lofi Girl", avatar: null, enabled: true, color: 3, addedAt: now,
    url: "https://www.youtube.com/@LofiGirl",
    timers: [], scheduled: false, timerActive: false, nextTimer: null,
    status: { live: false, checkedAt: now - 40_000, nextCheck: now + 50_000, checking: false, suppressed: false, force: false },
    session: null,
  },
];

const settings = {
  folder: "C:\\Users\\User\\Videos\\Rewind", quality: "best", pollSeconds: 90, holdMinutes: 10, autostart: false,
  startHidden: true, closeToTray: true, keepDays: 0, sounds: false, paused: false, autoUpdate: true,
};

const handlers: Record<string, (a: Args) => unknown> = {
  get_snapshot: () => ({ channels, paused: false, toolsReady: true, folder: settings.folder, now: Date.now() }),
  get_settings: () => settings,
  list_tapes: () => [],
  disk_info: () => ({ free: 268 * 1024 ** 3, total: 1000 * 1024 ** 3 }),
  tools_status: () => ({ ytDlp: "x", ytDlpVersion: "2026.11.23", ytDlpManaged: true, ffmpeg: "x", ffmpegVersion: "8.1.1" }),
  launch_info: () => ({ version: "1.0.3" }),
  probe_video: async (a) => {
    await new Promise((r) => setTimeout(r, 700));
    const MB = 1_048_576;
    const q = (id: string, height: number, fps: number, mb: number, codec: string, hdr = false) => ({ id, height, fps, size: mb * MB, codec, hdr });
    return {
      url: a.url, id: "LXb3EKWsInQ", title: "COSTA RICA IN 4K 60fps HDR (ULTRA HD)", channel: "Jacob + Katie Schwarz", duration: 314,
      thumbnail: null, site: "Youtube",
      qualities: [
        q("2160p60", 2160, 60, 904, "VP9"), q("1440p60", 1440, 60, 355, "VP9"), q("1080p60", 1080, 60, 163, "H.264"),
        q("720p60", 720, 60, 94, "H.264"), q("720p", 720, 30, 59, "H.264"), q("480p60", 480, 60, 73, "VP9", true),
        q("480p", 480, 30, 25, "H.264"), q("360p", 360, 30, 16, "H.264"), q("240p", 240, 30, 10, "H.264"),
        q("144p", 144, 30, 7, "H.264"), { id: "audio", height: 0, fps: 0, size: 4 * MB, codec: null, hdr: false },
      ],
    };
  },
  list_downloads: () => [
    { id: 2, url: "x", title: "Lofi Girl - 1 A.M Study Session", channel: "Lofi Girl", thumbnail: null, quality: "1080p 60 fps", state: "downloading", progress: 0.43, speed: 2.4 * 1_048_576, eta: 95, size: 180 * 1_048_576, startedAt: now },
    { id: 1, url: "y", title: "Building a VCR in Rust", channel: "Some Channel", thumbnail: null, quality: "720p", state: "done", progress: 1, size: 59 * 1_048_576, file: "C:\\x.mp4", startedAt: now },
  ],
  set_channel_timers: (a) => {
    const c = channels.find((x) => x.id === a.id);
    if (c) {
      c.timers = a.timers as typeof c.timers;
      c.scheduled = c.timers.some((t) => t.enabled && t.days);
    }
  },
};

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  transformCallback: () => Math.floor(Math.random() * 1e9),
  convertFileSrc: (p: string) => p,
  invoke: async (cmd: string, args: Args) => {
    if (cmd.startsWith("plugin:")) return 0;
    const handler = handlers[cmd];
    return handler ? handler(args ?? {}) : null;
  },
};
