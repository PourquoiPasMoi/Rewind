import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/deck.css";
import "./styles/channels.css";
import "./styles/tapes.css";
import "./styles/player.css";
import "./styles/setup.css";
import "./styles/timer.css";

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Snapshot, type UpdateInfo } from "./api";
import { ChannelsView } from "./channels";
import { Deck } from "./deck";
import { osd, type OsdKind } from "./osd";
import { SetupView } from "./setup";
import * as sound from "./sound";
import { TapesView } from "./tapes";
import { reducedMotion } from "./util";

const win = getCurrentWindow();
const deck = new Deck();
const channels = new ChannelsView(document.getElementById("view-channels")!);
const tapes = new TapesView(document.getElementById("view-tapes")!);
const setup = new SetupView(document.getElementById("view-setup")!);
let current = "channels";
let lastSnap: Snapshot | null = null;

// ---- window buttons
document.getElementById("win-min")!.addEventListener("click", () => win.minimize());
document.getElementById("win-max")!.addEventListener("click", () => win.toggleMaximize());
document.getElementById("win-close")!.addEventListener("click", () => win.close());
document.querySelector(".titlebar")!.addEventListener("dblclick", (e) => {
  if (!(e.target as HTMLElement).closest("button")) void win.toggleMaximize();
});

// ---- piano keys: switch views with a short tracking glitch
const keys = [...document.querySelectorAll<HTMLButtonElement>(".key")];
function show(view: string) {
  if (view === current) return;
  current = view;
  sound.click();
  for (const key of keys) key.setAttribute("aria-selected", String(key.dataset.view === view));
  const screen = document.getElementById("screen")!;
  const swap = () => {
    for (const section of document.querySelectorAll<HTMLElement>(".view")) section.hidden = section.dataset.view !== view;
    screen.scrollTop = 0;
  };
  if (reducedMotion()) swap();
  else {
    screen.classList.remove("tracking");
    void screen.offsetWidth;
    screen.classList.add("tracking");
    setTimeout(swap, 110);
    setTimeout(() => screen.classList.remove("tracking"), 360);
  }
  if (view === "tapes") void tapes.refresh();
  if (view === "setup") void setup.load();
}
keys.forEach((key) => key.addEventListener("click", () => show(key.dataset.view!)));
window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && ["1", "2", "3"].includes(e.key)) {
    e.preventDefault();
    show(["channels", "tapes", "setup"][Number(e.key) - 1]);
  }
});

// ---- watching switch
const pauseSwitch = document.getElementById("pause-switch") as HTMLButtonElement;
pauseSwitch.addEventListener("click", async () => {
  sound.click();
  const paused = !(lastSnap?.paused ?? false);
  await api.setPaused(paused).catch(() => {});
  osd(paused ? "Watching paused. Recordings already running continue." : "Watching your channels again", "info");
});
function paintPause(paused: boolean) {
  pauseSwitch.setAttribute("aria-pressed", String(!paused));
  pauseSwitch.classList.toggle("paused", paused);
  document.getElementById("pause-label")!.textContent = paused ? "Paused" : "Watching";
  pauseSwitch.title = paused ? "Not looking for live streams. Click to watch again." : "Looking for live streams. Click to pause.";
}

// ---- data
function applySnapshot(snap: Snapshot) {
  lastSnap = snap;
  deck.update(snap);
  channels.update(snap);
  tapes.setChannels(snap);
  paintPause(snap.paused);
  setup.setChannelCount(
    snap.channels.filter((c) => c.platform === "twitch").length,
    snap.channels.filter((c) => c.platform === "youtube").length,
  );
}

tapes.onCount = (n) => deck.setTapes(n);
setup.onSettings = (s) => sound.setSounds(s.sounds);

async function refreshDisk() {
  const disk = await api.disk().catch(() => null);
  deck.setFree(disk ? disk.free : null);
}

async function start() {
  const boot = document.getElementById("boot")!;
  const [snap, settings] = await Promise.all([api.snapshot(), api.settings()]);
  sound.setSounds(settings.sounds);
  applySnapshot(snap);
  void tapes.refresh();
  void refreshDisk();
  setInterval(refreshDisk, 60_000);

  await listen<Snapshot>("snapshot", (e) => applySnapshot(e.payload));
  await listen("tapes-changed", () => {
    void tapes.refresh();
    void refreshDisk();
  });
  await listen<{ kind: OsdKind; text: string }>("toast", (e) => osd(e.payload.text, e.payload.kind));
  await listen<UpdateInfo>("update-available", (e) => {
    const info = e.payload;
    setup.setUpdate(info);
    deck.setUpdate(info.version);
    // Tell once per version; after that the display and Setup keep showing it.
    let told = "";
    try {
      told = localStorage.getItem("rewind.toldUpdate") ?? "";
      localStorage.setItem("rewind.toldUpdate", info.version);
    } catch {
      /* storage unavailable: just tell again */
    }
    if (told !== info.version) osd(`Rewind ${info.version} is out. Install it from Setup.`, "info", 7000);
  });
  await listen<{ received?: number; total?: number | null; stage?: string }>("update-progress", (e) => setup.progress(e.payload));
  await listen<{ received: number; total: number | null }>("tool-progress", (e) => {
    const { received, total } = e.payload;
    const btn = document.querySelector<HTMLButtonElement>(".osd-btn.strong[disabled]");
    if (btn) btn.textContent = total ? `Downloading ${Math.round((received / total) * 100)}%` : `Downloading ${Math.round(received / 1048576)} MB`;
  });

  if (!snap.toolsReady) {
    // First run: send people to the helpers they need.
    setTimeout(() => {
      osd("Rewind needs two helper programs before it can record. They're in Setup.", "info", 7000);
    }, 2200);
  }

  boot.classList.add("on");
  await deck.boot();
  boot.remove();
}

void start();
