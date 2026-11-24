import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export type Platform = "twitch" | "youtube";

export interface Status {
  live: boolean;
  title?: string | null;
  game?: string | null;
  viewers?: number | null;
  startedAt?: string | null;
  thumbnail?: string | null;
  videoId?: string | null;
  upcomingAt?: number | null;
  checkedAt: number;
  nextCheck: number;
  checking: boolean;
  error?: string | null;
  suppressed: boolean;
  force: boolean;
}

export interface SessionView {
  state: "recording" | "holding" | "saving";
  startedAt: number;
  partStartedAt?: number | null;
  recordedSecs: number;
  bytes: number;
  parts: number;
  holdUntil: number;
  title: string;
}

export interface Timer {
  /** Bit 0 = Monday ... bit 6 = Sunday. */
  days: number;
  /** Minutes after midnight. */
  start: number;
  end: number;
  enabled: boolean;
}

export interface ChannelView {
  timers: Timer[];
  scheduled: boolean;
  timerActive: boolean;
  nextTimer?: number | null;
  id: string;
  platform: Platform;
  handle: string;
  name: string;
  avatar?: string | null;
  enabled: boolean;
  color: number;
  addedAt: number;
  url: string;
  status?: Status | null;
  session?: SessionView | null;
}

export interface Snapshot {
  channels: ChannelView[];
  paused: boolean;
  toolsReady: boolean;
  folder: string;
  now: number;
}

export interface Settings {
  folder: string;
  quality: "best" | "1080" | "720" | "480" | "audio";
  pollSeconds: number;
  holdMinutes: number;
  autostart: boolean;
  startHidden: boolean;
  closeToTray: boolean;
  keepDays: number;
  sounds: boolean;
  paused: boolean;
  autoUpdate: boolean;
}

export interface UpdateInfo {
  version: string;
  current: string;
  notes?: string | null;
}

export interface Tape {
  path: string;
  fileName: string;
  size: number;
  playable: boolean;
  thumb?: string | null;
  channelId: string;
  channelName: string;
  platform: string;
  title: string;
  game?: string | null;
  startedAt: number;
  endedAt: number;
  durationSecs: number;
  parts: number;
  url: string;
}

export interface Tools {
  ytDlp?: string | null;
  ytDlpVersion?: string | null;
  ytDlpManaged: boolean;
  ffmpeg?: string | null;
  ffmpegVersion?: string | null;
  node?: string | null;
}

export interface Quality {
  id: string;
  height: number;
  fps: number;
  size?: number | null;
  codec?: string | null;
  hdr: boolean;
}

export interface VideoInfo {
  url: string;
  id: string;
  title: string;
  channel: string;
  duration?: number | null;
  thumbnail?: string | null;
  site: string;
  qualities: Quality[];
}

export interface DownloadJob {
  id: number;
  url: string;
  title: string;
  channel: string;
  thumbnail?: string | null;
  quality: string;
  state: "downloading" | "joining" | "done" | "failed" | "cancelled";
  progress: number;
  speed?: number | null;
  eta?: number | null;
  size?: number | null;
  file?: string | null;
  error?: string | null;
  startedAt: number;
}

export const api = {
  probeVideo: (url: string) => invoke<VideoInfo>("probe_video", { url }),
  startDownload: (url: string, quality: string) => invoke<number>("start_download", { url, quality }),
  downloads: () => invoke<DownloadJob[]>("list_downloads"),
  cancelDownload: (id: number) => invoke<void>("cancel_download", { id }),
  clearDownloads: () => invoke<void>("clear_downloads"),
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  addChannel: (input: string) => invoke<ChannelView>("add_channel", { input }),
  removeChannel: (id: string) => invoke<void>("remove_channel", { id }),
  setEnabled: (id: string, enabled: boolean) => invoke<void>("set_channel_enabled", { id, enabled }),
  setColor: (id: string, color: number) => invoke<void>("set_channel_color", { id, color }),
  setTimers: (id: string, timers: Timer[]) => invoke<void>("set_channel_timers", { id, timers }),
  recordNow: (id: string) => invoke<void>("record_now", { id }),
  stop: (id: string) => invoke<void>("stop_recording", { id }),
  checkNow: (id?: string) => invoke<void>("check_now", { id: id ?? null }),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  tapes: () => invoke<Tape[]>("list_tapes"),
  deleteTape: (path: string) => invoke<void>("delete_tape", { path }),
  reveal: (path?: string) => invoke<void>("reveal", { path: path ?? null }),
  openExternal: (path: string) => invoke<void>("open_external", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  tools: () => invoke<Tools>("tools_status"),
  installYtDlp: () => invoke<Tools>("install_yt_dlp"),
  updateYtDlp: () => invoke<string>("update_yt_dlp"),
  installFfmpeg: () => invoke<void>("install_ffmpeg"),
  disk: () => invoke<{ free: number; total: number } | null>("disk_info"),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: (force: boolean) => invoke<void>("install_update", { force }),
  version: () => invoke<{ version: string }>("launch_info").then((r) => r.version),
  quit: () => invoke<void>("quit_app"),
  hide: () => invoke<void>("hide_window"),
};

export const fileUrl = (path: string) => convertFileSrc(path);

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong";
}
