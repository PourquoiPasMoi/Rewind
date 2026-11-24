// Channels view: a label maker to add channels, and one cassette per channel.

import { api, errorText, type ChannelView, type Snapshot } from "./api";
import { icon } from "./icons";
import { osd } from "./osd";
import * as sound from "./sound";
import { ago, bytes, clock, el, reducedMotion, svg, until } from "./util";

interface Card {
  root: HTMLElement;
  name: HTMLElement;
  sub: HTMLElement;
  photo: HTMLImageElement;
  status: HTMLElement;
  meta: HTMLElement;
  stamp: HTMLElement;
  primary: HTMLButtonElement;
  watch: HTMLButtonElement;
  data: ChannelView;
  photoSrc: string;
}

const COLORS = 6;

export class ChannelsView {
  private host: HTMLElement;
  private grid: HTMLElement;
  private empty: HTMLElement;
  private input!: HTMLInputElement;
  private form!: HTMLFormElement;
  private cards = new Map<string, Card>();
  private snapAt = performance.now();
  private now = Date.now();
  private justAdded: string | null = null;

  constructor(host: HTMLElement) {
    this.host = host;
    this.grid = el("div", { class: "cassette-grid" });
    this.empty = el("div", { class: "empty-shelf" },
      el("div", { class: "empty-cassette", "aria-hidden": "true" }, el("i"), el("i")),
      el("h2", { text: "No channels yet" }),
      el("p", { text: "Paste a channel link above. When it goes live, Rewind starts recording, and stops when the stream ends." }),
    );
    this.host.append(this.labeler(), this.grid, this.empty);
    setInterval(() => this.tick(), 1000);
  }

  private labeler(): HTMLElement {
    this.input = el("input", {
      class: "labeler-input",
      type: "text",
      placeholder: "twitch.tv/name or youtube.com/@name",
      "aria-label": "Channel link",
      spellcheck: "false",
      autocomplete: "off",
    });
    const button = el("button", { class: "punch", type: "submit" }, el("span", { text: "Add channel" }));
    this.form = el("form", { class: "labeler" },
      el("div", { class: "labeler-body" },
        el("span", { class: "labeler-dial", "aria-hidden": "true" }),
        el("div", { class: "labeler-tape" }, this.input),
      ),
      button,
    );
    this.form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const value = this.input.value.trim();
      if (!value) {
        this.input.focus();
        return;
      }
      sound.click();
      this.form.classList.add("busy");
      button.disabled = true;
      try {
        const channel = await api.addChannel(value);
        this.justAdded = channel.id;
        this.input.value = "";
        this.form.classList.remove("busy");
        this.form.classList.add("punched");
        setTimeout(() => this.form.classList.remove("punched"), 500);
        sound.insert();
        osd(`${channel.name} is on the shelf`, "info");
      } catch (e) {
        this.form.classList.remove("busy");
        this.form.classList.add("jam");
        setTimeout(() => this.form.classList.remove("jam"), 450);
        osd(errorText(e), "error");
      } finally {
        button.disabled = false;
      }
    });
    return el("div", { class: "labeler-wrap" },
      this.form,
      el("p", { class: "labeler-hint", text: "A Twitch name works too. Each channel is checked quietly in the background." }),
    );
  }

  update(snap: Snapshot) {
    this.snapAt = performance.now();
    this.now = snap.now;
    const seen = new Set<string>();
    snap.channels.forEach((channel, index) => {
      seen.add(channel.id);
      let card = this.cards.get(channel.id);
      if (!card) {
        card = this.build(channel);
        this.cards.set(channel.id, card);
        this.grid.append(card.root);
        if (this.justAdded === channel.id && !reducedMotion()) {
          card.root.classList.add("inserting");
          card.root.addEventListener("animationend", () => card!.root.classList.remove("inserting"), { once: true });
          this.justAdded = null;
        }
      }
      if (this.grid.children[index] !== card.root) this.grid.insertBefore(card.root, this.grid.children[index] ?? null);
      card.data = channel;
      this.paint(card);
    });
    for (const [id, card] of this.cards) {
      if (!seen.has(id)) {
        card.root.remove();
        this.cards.delete(id);
      }
    }
    this.empty.hidden = snap.channels.length > 0;
  }

  private build(channel: ChannelView): Card {
    const reel = (side: string) =>
      el("div", { class: `reel ${side}` },
        el("div", { class: "pack" }),
        svg(`<svg class="hub" viewBox="0 0 40 40" aria-hidden="true"><circle cx="20" cy="20" r="18" class="hub-ring"/><g class="hub-teeth">${[0, 60, 120, 180, 240, 300]
          .map((a) => `<rect x="18" y="5" width="4" height="7" rx="1" transform="rotate(${a} 20 20)"/>`)
          .join("")}</g><circle cx="20" cy="20" r="6" class="hub-core"/></svg>`),
      );
    const name = el("div", { class: "label-name" });
    const sub = el("div", { class: "label-sub" });
    const photo = el("img", { class: "photo-img", alt: "", loading: "lazy", draggable: "false" });
    const stamp = el("div", { class: "stamp", "aria-hidden": "true" });
    const status = el("div", { class: "card-status" });
    const meta = el("div", { class: "card-meta" });
    const primary = el("button", { class: "tape-btn primary" });
    const watch = el("button", { class: "tape-btn icon" });
    const open = el("button", { class: "tape-btn icon", title: "Open channel page", "aria-label": "Open channel page" });
    open.innerHTML = icon.external;
    const more = el("button", { class: "tape-btn icon", title: "Label colour and remove", "aria-label": "More" });
    more.innerHTML = icon.dots;

    const root = el("article", { class: "cassette-card", "data-id": channel.id },
      el("div", { class: "cassette" },
        el("span", { class: "screw s1" }), el("span", { class: "screw s2" }),
        el("span", { class: "screw s3" }), el("span", { class: "screw s4" }),
        el("div", { class: "label" },
          el("div", { class: "label-band" },
            el("span", { class: "label-side", text: "A" }),
            el("span", { class: "label-platform" }),
            el("span", { class: "label-brand", text: channel.platform === "twitch" ? "Twitch" : "YouTube" }),
          ),
          el("div", { class: "label-paper" }, name, sub),
          el("div", { class: "window" }, reel("left"), el("div", { class: "tape-run" }), reel("right")),
        ),
        el("div", { class: "head" }, el("span", { class: "head-hole" }), el("span", { class: "head-hole" }), el("span", { class: "head-led" })),
        el("figure", { class: "photo" }, photo, el("span", { class: "photo-tape" })),
        stamp,
      ),
      el("div", { class: "card-foot" },
        el("div", { class: "card-text" }, status, meta),
        el("div", { class: "card-actions" }, primary, watch, open, more),
      ),
    );
    (root.querySelector(".label-platform") as HTMLElement).innerHTML = channel.platform === "twitch" ? icon.twitch : icon.youtube;

    const card: Card = { root, name, sub, photo, status, meta, stamp, primary, watch, data: channel, photoSrc: "" };

    primary.addEventListener("click", async () => {
      sound.click();
      const c = card.data;
      try {
        if (c.session && c.session.state !== "saving") {
          await api.stop(c.id);
          sound.eject();
          osd(`Stopped ${c.name}. The tape is being saved.`, "info");
        } else {
          await api.recordNow(c.id);
          osd(`Looking at ${c.name} now`, "info");
        }
      } catch (e) {
        osd(errorText(e), "error");
      }
    });
    watch.addEventListener("click", async () => {
      sound.click();
      try {
        await api.setEnabled(card.data.id, !card.data.enabled);
      } catch (e) {
        osd(errorText(e), "error");
      }
    });
    open.addEventListener("click", () => api.openUrl(card.data.url).catch((e) => osd(errorText(e), "error")));
    more.addEventListener("click", (event) => {
      event.stopPropagation();
      this.menu(card, more);
    });
    photo.addEventListener("error", () => photo.closest(".photo")?.classList.add("broken"));
    photo.addEventListener("load", () => photo.closest(".photo")?.classList.remove("broken"));
    return card;
  }

  private paint(card: Card) {
    const c = card.data;
    const st = c.status;
    const s = c.session;
    const state = s ? s.state : st?.live ? "live" : !c.enabled ? "off" : st?.upcomingAt ? "soon" : "idle";
    const root = card.root;
    for (const cls of ["is-recording", "is-holding", "is-saving", "is-live", "is-off", "is-soon", "is-idle"]) root.classList.remove(cls);
    root.classList.add(`is-${state}`);
    root.style.setProperty("--label", `var(--c${c.color % COLORS})`);

    card.name.textContent = c.name;
    const subText = s?.title || (st?.live ? st.title : "") || (st?.upcomingAt ? "Scheduled stream" : "") || (c.platform === "twitch" ? `twitch.tv/${c.handle}` : c.handle);
    card.sub.textContent = subText ?? "";
    card.sub.title = subText ?? "";

    const photoSrc = (st?.live && st.thumbnail ? withBust(st.thumbnail) : c.avatar) ?? "";
    if (photoSrc !== card.photoSrc) {
      card.photoSrc = photoSrc;
      if (photoSrc) card.photo.src = photoSrc;
      card.photo.closest(".photo")!.classList.toggle("empty", !photoSrc);
    }
    card.photo.closest(".photo")!.classList.toggle("wide", Boolean(st?.live && st.thumbnail));

    card.stamp.textContent = state === "recording" ? "REC" : state === "holding" ? "Hold" : state === "saving" ? "Saving" : state === "live" ? "Live" : "";

    const recording = s && s.state !== "saving";
    card.primary.innerHTML = recording ? `${icon.stop}<span>Stop</span>` : `${icon.rec}<span>Record now</span>`;
    card.primary.disabled = s?.state === "saving";
    card.primary.classList.toggle("danger", Boolean(recording));
    card.primary.title = recording ? "Stop and save this tape" : "Check now and record if live";
    card.watch.innerHTML = c.enabled ? icon.eye : icon.eyeOff;
    card.watch.title = c.enabled ? "Watching: records automatically. Click to stop watching." : "Not watching. Click to watch again.";
    card.watch.setAttribute("aria-label", card.watch.title);
    card.watch.classList.toggle("off", !c.enabled);
    this.paintText(card);
  }

  /** Text that changes every second (timers). */
  private paintText(card: Card) {
    const c = card.data;
    const st = c.status;
    const s = c.session;
    const drift = (performance.now() - this.snapAt) / 1000;
    const now = this.now + drift * 1000;
    let status = "";
    let meta = "";
    let error = false;
    if (s?.state === "recording") {
      const secs = s.recordedSecs + drift;
      status = `Recording ${clock(secs)}`;
      meta = `${bytes(s.bytes)}${s.parts > 1 ? `, part ${s.parts}` : ""}${st?.viewers ? `, ${st.viewers.toLocaleString()} watching` : ""}`;
      card.root.style.setProperty("--wind", String(Math.min(1, (secs % 7200) / 7200)));
    } else if (s?.state === "holding") {
      status = "Stream dropped";
      meta = `Keeping the tape open ${until(s.holdUntil, now)} in case it comes back`;
    } else if (s?.state === "saving") {
      status = "Saving tape";
      meta = `${clock(s.recordedSecs)} recorded, ${bytes(s.bytes)}`;
    } else if (st?.live) {
      status = "Live now";
      if (st.suppressed) meta = "You stopped this stream. The next one records again.";
      else if (st.error) {
        meta = st.error;
        error = true;
      } else meta = c.enabled ? "Starting the recorder" : "Not watching this channel";
    } else if (!c.enabled) {
      status = "Not watching";
      meta = "Turn watching back on to record new streams";
    } else if (st?.error) {
      status = "Can't check";
      meta = st.error;
      error = true;
    } else if (st?.checking || !st?.checkedAt) {
      status = "Checking";
      meta = "Looking for a live stream";
    } else if (st.upcomingAt) {
      status = "Scheduled";
      meta = new Date(st.upcomingAt * 1000).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" });
    } else {
      status = "Off air";
      meta = `Checked ${ago(st.checkedAt, now)}`;
    }
    card.status.textContent = status;
    card.meta.textContent = meta;
    card.meta.title = meta;
    card.meta.classList.toggle("error", error);
  }

  private tick() {
    for (const card of this.cards.values()) this.paintText(card);
  }

  private menu(card: Card, anchor: HTMLElement) {
    document.querySelector(".pop")?.remove();
    const swatches = el("div", { class: "pop-swatches", role: "group", "aria-label": "Label colour" });
    for (let i = 0; i < COLORS; i++) {
      const sw = el("button", { class: "swatch", style: `--sw: var(--c${i})`, "aria-label": `Colour ${i + 1}`, "aria-pressed": String(card.data.color % COLORS === i) });
      sw.addEventListener("click", async () => {
        sound.click();
        await api.setColor(card.data.id, i).catch((e) => osd(errorText(e), "error"));
        pop.remove();
      });
      swatches.append(sw);
    }
    const remove = el("button", { class: "pop-remove" }, el("span", { text: "Remove channel" }));
    remove.insertAdjacentHTML("afterbegin", icon.trash);
    let armed = false;
    remove.addEventListener("click", async () => {
      if (!armed) {
        armed = true;
        remove.classList.add("armed");
        remove.querySelector("span")!.textContent = "Click again to remove";
        return;
      }
      try {
        await api.removeChannel(card.data.id);
        sound.eject();
        osd(`${card.data.name} removed. Its tapes stay on the shelf.`, "info");
      } catch (e) {
        osd(errorText(e), "error");
      }
      pop.remove();
    });
    const pop = el("div", { class: "pop", role: "dialog", "aria-label": `${card.data.name} options` },
      el("p", { class: "pop-title", text: "Label colour" }),
      swatches,
      remove,
    );
    document.body.append(pop);
    const r = anchor.getBoundingClientRect();
    const w = 220;
    pop.style.left = `${Math.min(window.innerWidth - w - 12, Math.max(12, r.right - w))}px`;
    pop.style.top = `${Math.min(window.innerHeight - 170, r.bottom + 8)}px`;
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent && event.key !== "Escape") return;
      if (event.type === "pointerdown" && pop.contains(event.target as Node)) return;
      pop.remove();
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("keydown", close, true);
    };
    setTimeout(() => {
      window.addEventListener("pointerdown", close, true);
      window.addEventListener("keydown", close, true);
    });
  }
}

/** Live previews change: ask for a fresh one every 5 minutes, not on every snapshot. */
function withBust(url: string): string {
  const slot = Math.floor(Date.now() / 300_000);
  return `${url}${url.includes("?") ? "&" : "?"}r=${slot}`;
}
