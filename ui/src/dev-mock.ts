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
