// Tapes view: every finished recording as a VHS box on a shelf, newest first.

import { api, errorText, fileUrl, type Snapshot, type Tape } from "./api";
import { icon } from "./icons";
import { osd } from "./osd";
import { openPlayer } from "./player";
import * as sound from "./sound";
import { bytes, el, longDuration } from "./util";

export class TapesView {
  private host: HTMLElement;
  private shelf: HTMLElement;
  private chips: HTMLElement;
  private summary: HTMLElement;
  private search: HTMLInputElement;
  private tapes: Tape[] = [];
  private filter = "";
  private query = "";
  private colors = new Map<string, number>();
  onCount: (count: number) => void = () => {};

  constructor(host: HTMLElement) {
    this.host = host;
    this.search = el("input", { class: "tape-search", type: "search", placeholder: "Find a tape", "aria-label": "Find a tape", spellcheck: "false" });
    this.search.addEventListener("input", () => {
      this.query = this.search.value.trim().toLowerCase();
      this.render();
    });
    const folder = el("button", { class: "plain-btn", title: "Open the recordings folder" });
    folder.innerHTML = `${icon.folder}<span>Open folder</span>`;
    folder.addEventListener("click", () => api.reveal().catch((e) => osd(errorText(e), "error")));
    this.chips = el("div", { class: "chips", role: "group", "aria-label": "Channel" });
    this.summary = el("span", { class: "shelf-summary" });
    const searchWrap = el("label", { class: "search-wrap" }, this.search);
    searchWrap.insertAdjacentHTML("afterbegin", icon.search);
    this.shelf = el("div", { class: "shelf" });
    this.host.append(el("div", { class: "tapes-bar" }, searchWrap, this.chips, el("div", { class: "bar-end" }, this.summary, folder)), this.shelf);
  }

  setChannels(snap: Snapshot) {
    this.colors = new Map(snap.channels.map((c) => [c.id, c.color]));
  }

  async refresh() {
    try {
      this.tapes = await api.tapes();
    } catch (e) {
      osd(errorText(e), "error");
      return;
    }
    this.onCount(this.tapes.length);
    this.render();
  }

  private render() {
    const names = [...new Set(this.tapes.map((t) => t.channelName).filter(Boolean))].sort((a, b) => a.localeCompare(b));
    if (this.filter && !names.includes(this.filter)) this.filter = "";
    this.chips.replaceChildren(
      ...(names.length > 1 ? ["", ...names] : []).map((name) => {
        const chip = el("button", { class: "chip", "aria-pressed": String(this.filter === name), text: name || "All channels" });
        chip.addEventListener("click", () => {
          sound.click();
          this.filter = name;
          this.render();
        });
        return chip;
      }),
    );

    const shown = this.tapes.filter(
      (t) =>
        (!this.filter || t.channelName === this.filter) &&
        (!this.query || `${t.title} ${t.channelName} ${t.game ?? ""}`.toLowerCase().includes(this.query)),
    );
    const total = shown.reduce((sum, t) => sum + t.size, 0);
    this.summary.textContent = shown.length ? `${shown.length} tape${shown.length === 1 ? "" : "s"}, ${bytes(total)}` : "";

    if (!this.tapes.length) {
      this.shelf.replaceChildren(
        el("div", { class: "empty-shelf" },
          el("div", { class: "empty-vhs", "aria-hidden": "true" }),
          el("h2", { text: "The shelf is empty" }),
          el("p", { text: "When a channel goes live, its recording lands here as soon as the stream ends." }),
        ),
      );
      return;
    }
    if (!shown.length) {
      this.shelf.replaceChildren(el("p", { class: "shelf-none", text: "No tape matches that search." }));
      return;
    }

    // One row per day: the shelf reads like a diary.
    const groups = new Map<string, Tape[]>();
    for (const tape of shown) {
      const key = dayLabel(tape.startedAt);
      groups.set(key, [...(groups.get(key) ?? []), tape]);
    }
    const rows: HTMLElement[] = [];
    let delay = 0;
    for (const [day, tapes] of groups) {
      rows.push(
        el("section", { class: "shelf-row" },
          el("h3", { class: "shelf-day", text: day }),
          el("div", { class: "shelf-boxes" }, ...tapes.map((t) => this.box(t, delay++))),
          el("div", { class: "shelf-plank", "aria-hidden": "true" }),
        ),
      );
    }
    this.shelf.replaceChildren(...rows);
  }

  private box(tape: Tape, index: number): HTMLElement {
    const color = this.colors.get(tape.channelId) ?? hash(tape.channelName) % 6;
    const cover = el("div", { class: "vhs-cover" });
    if (tape.thumb) {
      const img = el("img", { src: fileUrl(tape.thumb), alt: "", loading: "lazy", draggable: "false" });
      img.addEventListener("error", () => img.remove());
      cover.append(img);
    }
    const platform = el("span", { class: "vhs-platform" });
    platform.innerHTML = tape.platform === "youtube" ? icon.youtube : tape.platform === "twitch" ? icon.twitch : "";
    const time = new Date(tape.startedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    const box = el("button", {
      class: `vhs${tape.playable ? "" : " raw"}`,
      style: `--label: var(--c${color}); --i: ${Math.min(index, 18)}`,
      title: `${tape.title}\n${tape.channelName}`,
      "aria-label": `${tape.title}, ${tape.channelName}, ${time}`,
    },
      el("span", { class: "vhs-spine", "aria-hidden": "true" }),
      cover,
      el("span", { class: "vhs-band" }, platform, el("span", { class: "vhs-channel", text: tape.channelName || "Unknown" })),
      el("span", { class: "vhs-sticker" }, el("span", { class: "vhs-title", text: tape.title })),
      el("span", { class: "vhs-foot" },
        el("span", { class: "vhs-sp", text: tape.durationSecs ? longDuration(tape.durationSecs) : bytes(tape.size) }),
        el("span", { class: "vhs-time", text: time }),
      ),
    );
    box.addEventListener("click", () => {
      sound.insert();
      openPlayer(tape, () => this.refresh());
    });
    return box;
  }
}

function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
}

function hash(text: string): number {
  let h = 0;
  for (const ch of text) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return h;
}
