// Download tab: paste a video link, pick the quality, download. Finished videos join the tape shelf.

import { api, errorText, type DownloadJob, type Quality, type VideoInfo } from "./api";
import { icon } from "./icons";
import { osd } from "./osd";
import { openPlayer } from "./player";
import * as sound from "./sound";
import { bytes, clock, el } from "./util";

const PREF_KEY = "rewind.downloadQuality";

function rememberQuality(id: string) {
  try {
    localStorage.setItem(PREF_KEY, id);
  } catch {
    /* only a convenience */
  }
}

function preferredQuality(): string {
  try {
    return localStorage.getItem(PREF_KEY) ?? "1080p60";
  } catch {
    return "1080p60";
  }
}

/** Pick the remembered quality, else the closest one at or below it, else the best. */
function defaultQuality(qualities: Quality[]): Quality | undefined {
  const pref = preferredQuality();
  const exact = qualities.find((q) => q.id === pref);
  if (exact) return exact;
  if (pref === "audio") return qualities.find((q) => q.id === "audio");
  const height = parseInt(pref, 10) || 1080;
  return qualities.find((q) => q.height > 0 && q.height <= height) ?? qualities[0];
}

function qualityName(q: Quality): { big: string; small: string } {
  if (q.id === "audio") return { big: "Sound", small: "MP3 only" };
  const name = q.height >= 2160 ? "4K" : q.height >= 1440 ? "2K" : `${q.height}p`;
  return { big: q.height >= 1440 ? name : `${q.height}p`, small: `${q.height >= 1440 ? `${q.height}p ` : ""}${q.fps > 30 ? `${q.fps} fps` : "30 fps"}` };
}

const speedText = (bps?: number | null) => (bps ? `${(bps / 1_048_576).toFixed(bps > 10 * 1_048_576 ? 0 : 1)} MB/s` : "");

export class DownloadsView {
  private host: HTMLElement;
  private input!: HTMLInputElement;
  private lookup!: HTMLButtonElement;
  private result: HTMLElement;
  private list: HTMLElement;
  private listHead: HTMLElement;
  private info: VideoInfo | null = null;
  private chosen: Quality | null = null;
  onCount: (active: number) => void = () => {};

  constructor(host: HTMLElement) {
    this.host = host;
    this.result = el("div", { class: "dub-result" });
    this.list = el("div", { class: "dub-list" });
    const clear = el("button", { class: "plain-btn small", text: "Clear finished" });
    clear.addEventListener("click", () => api.clearDownloads());
    this.listHead = el("div", { class: "dub-list-head" }, el("h3", { text: "Downloads" }), clear);
    this.host.append(this.form(), this.result, this.listHead, this.list);
    this.renderResult();
    void api.downloads().then((jobs) => this.setJobs(jobs));
  }

  private form(): HTMLElement {
    this.input = el("input", {
      class: "dub-input",
      type: "text",
      placeholder: "Paste a YouTube video link",
      "aria-label": "Video link",
      spellcheck: "false",
      autocomplete: "off",
    });
    this.lookup = el("button", { class: "punch", type: "submit" }, el("span", { text: "Look up" }));
    const form = el("form", { class: "dub-form" },
      el("div", { class: "dub-slot" }, el("span", { class: "dub-slot-mark", "aria-hidden": "true" }), this.input),
      this.lookup,
    );
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      void this.probe();
    });
    // Pasting a link looks it up right away.
    this.input.addEventListener("paste", () => setTimeout(() => {
      if (/^https?:\/\//.test(this.input.value.trim())) void this.probe();
    }, 0));
    return el("div", { class: "dub-wrap" },
      form,
      el("p", { class: "labeler-hint", text: "Pick any quality the video has, from 4K 60 fps down to 144p. For live streams, add the channel on Channels instead." }),
    );
  }

  private async probe() {
    const url = this.input.value.trim();
    if (!url) {
      this.input.focus();
      return;
    }
    sound.click();
    this.lookup.disabled = true;
    this.info = null;
    this.chosen = null;
    this.result.replaceChildren(el("div", { class: "dub-loading" }, el("span", { class: "dub-reel", "aria-hidden": "true" }), el("span", { text: "Reading the video's qualities" })));
    try {
      this.info = await api.probeVideo(url);
      this.chosen = defaultQuality(this.info.qualities) ?? null;
      sound.insert();
    } catch (e) {
      osd(errorText(e), "error");
      this.result.replaceChildren(el("p", { class: "dub-error", text: errorText(e) }));
      this.lookup.disabled = false;
      return;
    }
    this.lookup.disabled = false;
    this.renderResult();
  }

  private renderResult() {
    const info = this.info;
    if (!info) {
      if (!this.result.firstChild) this.result.replaceChildren();
      return;
    }
    const thumb = el("div", { class: "dub-thumb" });
    if (info.thumbnail) thumb.append(el("img", { src: info.thumbnail, alt: "", draggable: "false" }));
    if (info.duration) thumb.append(el("span", { class: "dub-len", text: clock(info.duration) }));

    const grid = el("div", { class: "quality-grid", role: "radiogroup", "aria-label": "Quality" });
    for (const q of info.qualities) {
      const name = qualityName(q);
      const btn = el("button", {
        class: `quality${q.id === "audio" ? " audio" : ""}`,
        role: "radio",
        "aria-checked": String(this.chosen?.id === q.id),
        title: `${name.big} ${name.small}${q.codec ? `, ${q.codec}` : ""}${q.hdr ? ", HDR only" : ""}`,
      },
        el("span", { class: "q-big", text: name.big }),
        el("span", { class: "q-small", text: name.small }),
        el("span", { class: "q-size", text: q.size ? `≈ ${bytes(q.size)}` : "size unknown" }),
        q.hdr ? el("span", { class: "q-tag", text: "HDR" }) : null,
      );
      btn.addEventListener("click", () => {
        sound.click();
        this.chosen = q;
        rememberQuality(q.id);
        this.renderResult();
      });
      grid.append(btn);
    }

    const q = this.chosen;
    const go = el("button", { class: "dub-go" });
    go.innerHTML = `${icon.rec}<span>${q ? `Download ${q.id === "audio" ? "sound only" : qualityName(q).big + " " + qualityName(q).small}${q.size ? ` (≈ ${bytes(q.size)})` : ""}` : "Pick a quality"}</span>`;
    go.disabled = !q;
    go.addEventListener("click", async () => {
      if (!this.chosen || !this.info) return;
      sound.insert();
      go.disabled = true;
      try {
        await api.startDownload(this.info.url, this.chosen.id);
        osd(`Downloading ${this.info.title}`, "info");
        this.info = null;
        this.input.value = "";
        this.result.replaceChildren();
      } catch (e) {
        osd(errorText(e), "error");
        go.disabled = false;
      }
    });

    this.result.replaceChildren(
      el("article", { class: "dub-card" },
        thumb,
        el("div", { class: "dub-info" },
          el("p", { class: "dub-channel", text: info.channel }),
          el("h2", { class: "dub-title", text: info.title }),
          el("p", { class: "dub-pick", text: "Pick a quality" }),
          grid,
          el("div", { class: "dub-actions" }, go),
        ),
      ),
    );
  }

  setJobs(jobs: DownloadJob[]) {
    const active = jobs.filter((j) => j.state === "downloading" || j.state === "joining").length;
    this.onCount(active);
    this.listHead.hidden = jobs.length === 0;
    this.list.replaceChildren(...jobs.map((j) => this.row(j)));
  }

  private row(job: DownloadJob): HTMLElement {
    const busy = job.state === "downloading" || job.state === "joining";
    const pct = Math.round(job.progress * 100);
    const status =
      job.state === "downloading" ? [`${pct}%`, speedText(job.speed), job.eta ? `${clock(job.eta)} left` : ""].filter(Boolean).join("  ")
        : job.state === "joining" ? "Joining picture and sound"
        : job.state === "done" ? `Done, ${bytes(job.size ?? 0)}`
        : job.state === "cancelled" ? "Cancelled"
        : job.error ?? "Failed";
    const actions = el("div", { class: "dub-row-actions" });
    if (busy) {
      const cancel = el("button", { class: "tape-btn" }, el("span", { text: "Cancel" }));
      cancel.addEventListener("click", () => {
        sound.eject();
        void api.cancelDownload(job.id);
      });
      actions.append(cancel);
    } else if (job.state === "done" && job.file) {
      const play = el("button", { class: "tape-btn" });
      play.innerHTML = `${icon.play}<span>Play</span>`;
      play.addEventListener("click", async () => {
        const tape = (await api.tapes()).find((t) => t.path === job.file);
        if (tape) {
          sound.insert();
          openPlayer(tape, () => {});
        } else osd("That file is no longer in the folder", "error");
      });
      const folder = el("button", { class: "tape-btn icon", title: "Show in folder", "aria-label": "Show in folder" });
      folder.innerHTML = icon.folder;
      folder.addEventListener("click", () => api.reveal(job.file!).catch((e) => osd(errorText(e), "error")));
      actions.append(play, folder);
    }
    const thumb = el("div", { class: "dub-row-thumb" });
    if (job.thumbnail) thumb.append(el("img", { src: job.thumbnail, alt: "", loading: "lazy", draggable: "false" }));
    return el("div", { class: `dub-row is-${job.state}` },
      thumb,
      el("div", { class: "dub-row-main" },
        el("div", { class: "dub-row-title", text: job.title, title: job.title }),
        el("div", { class: "dub-row-sub", text: `${job.channel ? `${job.channel}, ` : ""}${job.quality}` }),
        el("div", { class: "dub-bar", role: "progressbar", "aria-valuenow": String(pct), "aria-valuemin": "0", "aria-valuemax": "100" },
          el("span", { style: `width: ${job.state === "done" ? 100 : pct}%` }),
        ),
        el("div", { class: `dub-row-status${job.state === "failed" ? " error" : ""}`, text: status }),
      ),
      actions,
    );
  }
}
