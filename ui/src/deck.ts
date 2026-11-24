// The VFD display on the front of the deck: clock, flags, a ticker and four counters.

import type { Snapshot } from "./api";
import { SegmentClock } from "./segments";
import { whenLabel } from "./timer";
import { bytes, clock, reducedMotion, until } from "./util";

const $ = (id: string) => document.getElementById(id)!;

export class Deck {
  private seg = new SegmentClock($("vfd-clock"));
  private snap: Snapshot | null = null;
  private snapAt = performance.now();
  private messages: string[] = ["Hello"];
  private msgIndex = 0;
  private booting = true;
  private tapes = 0;
  private newVersion: string | null = null;

  setUpdate(version: string | null) {
    this.newVersion = version;
    this.messages = this.buildMessages();
  }

  constructor() {
    this.seg.show("1200");
    setInterval(() => this.everySecond(), 1000);
    setInterval(() => this.rotate(), 4500);
  }

  /** Power-on: the classic blinking 12:00, then the real time. */
  async boot() {
    const fast = reducedMotion();
    this.seg.show("1200");
    this.seg.colonOn(true);
    this.seg.blink(!fast);
    this.setTicker("Hello");
    await new Promise((r) => setTimeout(r, fast ? 200 : 1700));
    this.seg.blink(false);
    this.booting = false;
    this.everySecond();
  }

  update(snap: Snapshot) {
    this.snap = snap;
    this.snapAt = performance.now();
    const recording = snap.channels.filter((c) => c.session?.state === "recording");
    const live = snap.channels.filter((c) => c.status?.live);
    $("count-live").textContent = String(live.length);
    $("count-rec").textContent = String(recording.length);
    $("flag-rec").classList.toggle("on", recording.length > 0);
    // Like a real VCR: TIMER lights when a timer is programmed, blinks while one is running.
    const timed = snap.channels.filter((c) => c.enabled && c.scheduled);
    $("flag-watch").classList.toggle("on", !snap.paused && timed.length > 0);
    $("flag-watch").classList.toggle("running", !snap.paused && timed.some((c) => c.timerActive));
    $("flag-pause").classList.toggle("on", snap.paused);
    document.getElementById("slot")!.classList.toggle("loaded", recording.length > 0);
    this.messages = this.buildMessages();
    if (!this.booting) this.renderTicker();
  }

  setTapes(count: number) {
    this.tapes = count;
    $("count-tapes").textContent = String(count);
  }

  setFree(free: number | null) {
    $("count-free").textContent = free === null ? "--" : bytes(free);
  }

  private elapsed(): number {
    return (performance.now() - this.snapAt) / 1000;
  }

  private buildMessages(): string[] {
    const snap = this.snap;
    if (!snap) return ["Hello"];
    const out: string[] = [];
    for (const c of snap.channels) {
      const s = c.session;
      if (s?.state === "recording") out.push(`● ${c.name}  ${clock(s.recordedSecs + this.elapsed())}`);
      else if (s?.state === "holding") out.push(`${c.name} dropped. Tape open ${until(s.holdUntil)}`);
      else if (s?.state === "saving") out.push(`Saving ${c.name}`);
    }
    if (!snap.toolsReady) out.push("Setup needs yt-dlp and ffmpeg");
    if (out.length === 0) {
      if (snap.channels.length === 0) out.push("Add a channel to start");
      else if (snap.paused) out.push("Watching paused");
      else {
        const watching = snap.channels.filter((c) => c.enabled).length;
        out.push(`Watching ${watching} channel${watching === 1 ? "" : "s"}`);
        const running = snap.channels.filter((c) => c.enabled && c.scheduled && c.timerActive);
        for (const c of running) out.push(`Timer on: ${c.name}`);
        const nextTimed = snap.channels
          .filter((c) => c.enabled && c.scheduled && !c.timerActive && c.nextTimer)
          .sort((a, b) => a.nextTimer! - b.nextTimer!)[0];
        if (nextTimed) out.push(`Timer: ${nextTimed.name} ${whenLabel(nextTimed.nextTimer!)}`);
        const next = snap.channels
          .filter((c) => c.enabled && c.status?.nextCheck && (!c.scheduled || c.timerActive))
          .map((c) => c.status!.nextCheck)
          .sort((a, b) => a - b)[0];
        if (next) out.push(`Next look in ${until(next)}`);
        if (this.tapes) out.push(`${this.tapes} tape${this.tapes === 1 ? "" : "s"} on the shelf`);
      }
    }
    if (this.newVersion) out.push(`Update ${this.newVersion} ready in Setup`);
    return out;
  }

  private everySecond() {
    if (this.booting) return;
    const now = new Date();
    const hh = String(now.getHours()).padStart(2, " ");
    const mm = String(now.getMinutes()).padStart(2, "0");
    this.seg.show(hh + mm);
    this.seg.colonOn(now.getSeconds() % 2 === 0);
    // Keep the running REC timer live between snapshots.
    const current = this.messages[this.msgIndex % this.messages.length] ?? "";
    if (current.startsWith("●")) {
      this.messages = this.buildMessages();
      this.renderTicker(false);
    }
  }

  private rotate() {
    if (this.booting || this.messages.length < 2) return;
    this.msgIndex = (this.msgIndex + 1) % this.messages.length;
    this.renderTicker();
  }

  private renderTicker(animate = true) {
    const text = this.messages[this.msgIndex % this.messages.length] ?? "";
    this.setTicker(text, animate);
  }

  private setTicker(text: string, animate = true) {
    const node = $("vfd-ticker");
    if (node.textContent === text) return;
    node.textContent = text;
    if (animate && !reducedMotion()) {
      node.classList.remove("flip");
      void node.offsetWidth;
      node.classList.add("flip");
    }
    const box = node.parentElement!;
    const over = node.scrollWidth - box.clientWidth;
    node.classList.toggle("scroll", over > 4);
    node.style.setProperty("--over", `${-over - 12}px`);
  }
}
