// Setup: the blue on-screen menu every VCR had. Arrow keys move the bar, left/right change values.

import { api, errorText, type Settings, type Tools, type UpdateInfo } from "./api";
import { osd } from "./osd";
import * as sound from "./sound";
import { el } from "./util";

type Option<T> = { value: T; label: string };

const QUALITY: Option<Settings["quality"]>[] = [
  { value: "best", label: "Best" },
  { value: "1080", label: "1080p" },
  { value: "720", label: "720p" },
  { value: "480", label: "480p" },
  { value: "audio", label: "Sound only" },
];
const GB_PER_HOUR: Record<Settings["quality"], number> = { best: 3, "1080": 2.7, "720": 1.5, "480": 0.7, audio: 0.07 };
const POLL: Option<number>[] = [
  { value: 30, label: "30 sec" },
  { value: 60, label: "1 min" },
  { value: 90, label: "90 sec" },
  { value: 120, label: "2 min" },
  { value: 300, label: "5 min" },
];
const HOLD: Option<number>[] = [
  { value: 0, label: "Off" },
  { value: 5, label: "5 min" },
  { value: 10, label: "10 min" },
  { value: 20, label: "20 min" },
  { value: 30, label: "30 min" },
];
const KEEP: Option<number>[] = [
  { value: 0, label: "Never" },
  { value: 7, label: "7 days" },
  { value: 30, label: "30 days" },
  { value: 90, label: "90 days" },
];
const ON_OFF: Option<boolean>[] = [
  { value: true, label: "On" },
  { value: false, label: "Off" },
];

export class SetupView {
  private host: HTMLElement;
  private settings: Settings | null = null;
  private tools: Tools | null = null;
  private channelCount = { twitch: 0, youtube: 0 };
  private menu!: HTMLElement;
  private rows: HTMLElement[] = [];
  private active = 0;
  /** Last helper download error: stays on screen (a toast is too easy to miss). */
  private toolError: string | null = null;
  private version = "";
  private update: UpdateInfo | null = null;
  private updateBusy = false;
  private updateNote: string | null = null;
  private updateArmed = false;
  onSettings: (s: Settings) => void = () => {};

  constructor(host: HTMLElement) {
    this.host = host;
    this.host.addEventListener("keydown", (e) => this.onKey(e));
  }

  setChannelCount(twitch: number, youtube: number) {
    const changed = twitch !== this.channelCount.twitch || youtube !== this.channelCount.youtube;
    this.channelCount = { twitch, youtube };
    if (changed && this.settings) this.render();
  }

  setUpdate(info: UpdateInfo | null) {
    this.update = info;
    if (this.settings) this.render();
  }

  /** Download / save / install progress from the Rust side. */
  progress(p: { received?: number; total?: number | null; stage?: string }) {
    const btn = this.host.querySelector<HTMLButtonElement>(".update-btn");
    if (!btn) return;
    if (p.stage === "saving") btn.textContent = "Saving your tapes";
    else if (p.stage === "installing") btn.textContent = "Installing, Rewind restarts";
    else if (p.received !== undefined) {
      btn.textContent = p.total ? `Downloading ${Math.round((p.received / p.total) * 100)}%` : `Downloading ${Math.round(p.received / 1048576)} MB`;
    }
  }

  async load() {
    try {
      this.version ||= await api.version();
      const [settings, tools] = await Promise.all([api.settings(), api.tools()]);
      this.settings = settings;
      this.tools = tools;
      this.onSettings(settings);
      this.render();
    } catch (e) {
      osd(errorText(e), "error");
    }
  }

  private async save(patch: Partial<Settings>) {
    if (!this.settings) return;
    const next = { ...this.settings, ...patch };
    this.settings = next;
    this.render();
    try {
      this.settings = await api.saveSettings(next);
      this.onSettings(this.settings);
      this.render();
    } catch (e) {
      osd(errorText(e), "error");
      await this.load();
    }
  }

  private render() {
    const s = this.settings;
    if (!s) return;
    const focusedKey = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>(".menu-row")?.dataset.key;
    this.rows = [];
    const folderBtn = el("button", { class: "osd-btn", text: "Change" });
    folderBtn.addEventListener("click", async () => {
      sound.click();
      const picked = await api.pickFolder().catch((e) => {
        osd(errorText(e), "error");
        return null;
      });
      if (picked) await this.save({ folder: picked });
    });
    const openFolder = el("button", { class: "osd-btn", text: "Open" });
    openFolder.addEventListener("click", () => api.reveal().catch((e) => osd(errorText(e), "error")));

    const checksPerDay = 86400 / s.pollSeconds;
    const checkMb = (checksPerDay * (this.channelCount.twitch * 1.5 + this.channelCount.youtube * 12)) / 1024;

    this.menu = el("div", { class: "osd-menu" },
      el("div", { class: "osd-head" }, el("h2", { text: "Setup" }), el("span", { class: "osd-keys", text: "↑↓ select   ←→ change" })),
      el("div", { class: "osd-group" },
        this.row("folder", "Save tapes in", el("div", { class: "osd-folder" }, el("span", { class: "osd-path", text: s.folder, title: s.folder }), folderBtn, openFolder)),
        this.choice("quality", "Picture quality", QUALITY, s.quality, (v) => this.save({ quality: v }),
          `About ${GB_PER_HOUR[s.quality]} GB of internet and disk per hour of stream`),
        this.choice("poll", "Look for live streams every", POLL, s.pollSeconds, (v) => this.save({ pollSeconds: v }),
          this.channelCount.twitch + this.channelCount.youtube
            ? `Looking uses about ${checkMb < 1 ? "under 1" : Math.round(checkMb)} MB a day for your channels`
            : "Twitch looks are tiny; YouTube looks are a bit bigger"),
        this.choice("hold", "If a stream drops, keep the tape open", HOLD, s.holdMinutes, (v) => this.save({ holdMinutes: v }),
          "A stream that comes back in time continues on the same tape"),
      ),
      el("div", { class: "osd-group" },
        this.choice("autostart", "Start with Windows", ON_OFF, s.autostart, (v) => this.save({ autostart: v })),
        this.choice("hidden", "Start hidden in the tray", ON_OFF, s.startHidden, (v) => this.save({ startHidden: v }),
          "Only when Windows starts Rewind"),
        this.choice("tray", "Close button", [{ value: true, label: "Hide to tray" }, { value: false, label: "Quit" }], s.closeToTray,
          (v) => this.save({ closeToTray: v }), s.closeToTray ? "Rewind keeps recording from the tray" : "Closing stops recording"),
        this.choice("keep", "Delete old tapes after", KEEP, s.keepDays, (v) => this.save({ keepDays: v }),
          s.keepDays ? "Old tapes are deleted for good, not moved to the recycle bin" : "Tapes stay until you delete them"),
        this.choice("sounds", "Button sounds", ON_OFF, s.sounds, (v) => this.save({ sounds: v })),
      ),
      this.updates(s),
      this.helpers(),
      el("div", { class: "osd-foot" }, this.quitButton()),
    );
    this.host.replaceChildren(this.menu);
    const again = focusedKey ? this.rows.findIndex((r) => r.dataset.key === focusedKey) : -1;
    if (again >= 0) {
      this.active = again;
      this.rows[again].focus({ preventScroll: true });
    }
    this.highlight();
  }

  private row(key: string, label: string, control: HTMLElement, note?: string): HTMLElement {
    const row = el("div", { class: "menu-row", tabindex: "0", "data-key": key },
      el("span", { class: "menu-label", text: label }),
      el("span", { class: "menu-dots", "aria-hidden": "true" }),
      el("div", { class: "menu-control" }, control),
      note ? el("span", { class: "menu-note", text: note }) : null,
    );
    row.addEventListener("focus", () => {
      this.active = this.rows.indexOf(row);
      this.highlight();
    });
    row.addEventListener("pointerenter", () => {
      this.active = this.rows.indexOf(row);
      this.highlight();
    });
    this.rows.push(row);
    return row;
  }

  private choice<T>(key: string, label: string, options: Option<T>[], current: T, set: (v: T) => void, note?: string): HTMLElement {
    const group = el("div", { class: "osd-options", role: "radiogroup", "aria-label": label });
    for (const option of options) {
      const btn = el("button", { class: "osd-option", role: "radio", "aria-checked": String(option.value === current), tabindex: "-1", text: option.label });
      btn.addEventListener("click", () => {
        if (option.value === current) return;
        sound.click();
        set(option.value);
      });
      group.append(btn);
    }
    const row = this.row(key, label, group, note);
    (row as HTMLElement & { step?: (d: number) => void }).step = (dir: number) => {
      const i = options.findIndex((o) => o.value === current);
      const next = options[Math.max(0, Math.min(options.length - 1, i + dir))];
      if (next && next.value !== current) {
        sound.click();
        set(next.value);
      }
    };
    return row;
  }

  private updates(s: Settings): HTMLElement {
    const control = el("div", { class: "osd-folder" });
    let note: string;
    if (this.update) {
      const install = el("button", { class: "osd-btn strong update-btn", text: this.updateArmed ? "Stop recording and update" : "Install update" });
      install.disabled = this.updateBusy;
      install.addEventListener("click", async () => {
        sound.click();
        this.updateBusy = true;
        this.updateNote = null;
        install.disabled = true;
        install.textContent = "Downloading";
        try {
          await api.installUpdate(this.updateArmed);
        } catch (e) {
          const text = errorText(e);
          this.updateBusy = false;
          if (text === "recording") {
            this.updateArmed = true;
            this.updateNote = "A stream is being recorded. Updating stops it and saves the tape first. Click again to update now.";
          } else {
            this.updateArmed = false;
            this.updateNote = text;
          }
          this.render();
        }
      });
      control.append(el("span", { class: "osd-ok", text: `Version ${this.update.version} is ready` }), install);
      const notes = (this.update.notes ?? "").replace(/[*#`]/g, "").split("\n").map((l) => l.trim()).find((l) => l.length > 0);
      note = `You have ${this.version}.${notes ? ` ${notes}` : ""}`;
    } else {
      const check = el("button", { class: "osd-btn", text: this.updateBusy ? "Checking" : "Check now" });
      check.disabled = this.updateBusy;
      check.addEventListener("click", async () => {
        sound.click();
        this.updateBusy = true;
        this.updateNote = null;
        this.render();
        try {
          this.update = await api.checkUpdate();
          if (!this.update) this.updateNote = "You have the newest version.";
        } catch (e) {
          this.updateNote = errorText(e);
        }
        this.updateBusy = false;
        this.render();
      });
      control.append(el("span", { text: `Version ${this.version}` }), check);
      note = "Updates come from Rewind's GitHub page and are checked for Rewind's signature before installing";
    }
    const row = this.row("update", "Rewind version", control, note);
    if (this.updateNote) {
      const good = this.updateNote.startsWith("You have the newest");
      row.append(el("span", { class: good ? "menu-note" : "menu-note menu-error", role: "status", text: this.updateNote }));
    }
    return el("div", { class: "osd-group" },
      el("h3", { class: "osd-sub", text: "Updates" }),
      row,
      this.choice("autoupdate", "Check for updates automatically", ON_OFF, s.autoUpdate, (v) => this.save({ autoUpdate: v }),
        "A tiny check (about 1 KB) when Rewind starts and every 6 hours. Nothing installs without your click."),
    );
  }

  private helpers(): HTMLElement {
    const t = this.tools;
    const group = el("div", { class: "osd-group helpers" }, el("h3", { class: "osd-sub", text: "Helper programs" }));
    if (!t) return group;

    const ytControl = el("div", { class: "osd-folder" });
    if (t.ytDlp) {
      ytControl.append(el("span", { class: "osd-ok", text: `Ready, version ${t.ytDlpVersion ?? "?"}` }));
      if (t.ytDlpManaged) {
        const update = el("button", { class: "osd-btn", text: "Update" });
        update.addEventListener("click", async () => {
          sound.click();
          update.textContent = "Updating";
          update.disabled = true;
          try {
            osd(await api.updateYtDlp(), "info");
          } catch (e) {
            osd(errorText(e), "error");
          }
          await this.load();
        });
        ytControl.append(update);
      }
    } else {
      const get = el("button", { class: "osd-btn strong", text: "Download yt-dlp (about 24 MB)" });
      get.addEventListener("click", async () => {
        sound.click();
        get.disabled = true;
        get.textContent = "Downloading";
        this.toolError = null;
        try {
          this.tools = await api.installYtDlp();
          osd("yt-dlp is ready", "saved");
        } catch (e) {
          this.toolError = errorText(e);
          osd(this.toolError, "error");
        }
        this.render();
      });
      ytControl.append(el("span", { class: "osd-bad", text: "Missing" }), get);
    }
    const ytRow = this.row("ytdlp", "yt-dlp, grabs the stream", ytControl,
      t.ytDlp ? "Twitch and YouTube change often. Update yt-dlp if recordings start failing." : "Downloaded from the official yt-dlp page on GitHub. A slow connection is fine, it resumes if it drops.");
    if (this.toolError && !t.ytDlp) {
      ytRow.append(el("span", { class: "menu-note menu-error", role: "alert", text: this.toolError }));
    }
    group.append(ytRow);

    const ffControl = el("div", { class: "osd-folder" });
    if (t.ffmpeg) {
      ffControl.append(el("span", { class: "osd-ok", text: `Ready, version ${t.ffmpegVersion ?? "?"}` }));
    } else {
      const get = el("button", { class: "osd-btn strong", text: "Install with winget" });
      get.addEventListener("click", async () => {
        sound.click();
        await api.installFfmpeg().catch((e) => osd(errorText(e), "error"));
        osd("Finish the install in the window that opened, then press Look again", "info");
      });
      const again = el("button", { class: "osd-btn", text: "Look again" });
      again.addEventListener("click", () => this.load());
      ffControl.append(el("span", { class: "osd-bad", text: "Missing" }), get, again);
    }
    group.append(this.row("ffmpeg", "ffmpeg, writes the video file", ffControl, t.ffmpeg ? undefined : "About 100 MB, installed by Windows' own winget"));
    return group;
  }

  private quitButton(): HTMLElement {
    const quit = el("button", { class: "osd-btn quit", text: "Quit Rewind" });
    quit.addEventListener("click", () => {
      sound.eject();
      void api.quit();
    });
    return el("div", { class: "osd-quit" }, quit, el("span", { class: "menu-note", text: "Open recordings are saved before Rewind closes. While closed, nothing is recorded." }));
  }

  private highlight() {
    this.rows.forEach((row, i) => row.classList.toggle("active", i === this.active));
  }

  private onKey(event: KeyboardEvent) {
    if (!this.rows.length || (event.target as HTMLElement).tagName === "INPUT") return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.active = (this.active + (event.key === "ArrowDown" ? 1 : -1) + this.rows.length) % this.rows.length;
      this.rows[this.active].focus();
      this.highlight();
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      const row = this.rows[this.active] as HTMLElement & { step?: (d: number) => void };
      if (row?.step) {
        event.preventDefault();
        row.step(event.key === "ArrowRight" ? 1 : -1);
      }
    }
  }
}
