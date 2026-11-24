// The TV: plays a tape with VCR-style on-screen text, and remembers where you stopped.

import { api, errorText, fileUrl, type Tape } from "./api";
import { icon } from "./icons";
import { osd } from "./osd";
import * as sound from "./sound";
import { bytes, clock, dateLabel, el, longDuration, reducedMotion } from "./util";

const RESUME_KEY = "rewind.resume";

function readResume(): Record<string, number> {
  try {
    return JSON.parse(localStorage.getItem(RESUME_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function saveResume(path: string, time: number | null) {
  try {
    const all = readResume();
    if (time === null) delete all[path];
    else all[path] = Math.floor(time);
    const keys = Object.keys(all);
    if (keys.length > 200) delete all[keys[0]];
    localStorage.setItem(RESUME_KEY, JSON.stringify(all));
  } catch {
    /* storage can be unavailable; resume is a convenience */
  }
}

/** A burst of analog snow while the "tape" loads. */
function snow(canvas: HTMLCanvasElement, ms: number) {
  const ctx = canvas.getContext("2d");
  if (!ctx || reducedMotion()) return;
  canvas.width = 160;
  canvas.height = 90;
  const img = ctx.createImageData(160, 90);
  const end = performance.now() + ms;
  const frame = () => {
    for (let i = 0; i < img.data.length; i += 4) {
      const v = Math.random() * 255;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
    if (performance.now() < end) requestAnimationFrame(frame);
    else canvas.classList.add("gone");
  };
  canvas.classList.remove("gone");
  frame();
}

export function openPlayer(tape: Tape, onChange: () => void) {
  document.querySelector(".player")?.remove();
  const video = el("video", { class: "tv-video", controls: true, preload: "metadata", playsinline: true });
  const canvas = el("canvas", { class: "tv-snow", "aria-hidden": "true" });
  const osdTop = el("div", { class: "tv-osd top", "aria-hidden": "true" });
  const osdCounter = el("div", { class: "tv-osd counter", "aria-hidden": "true" });
  const screen = el("div", { class: "tv-screen" }, video, canvas, osdTop, osdCounter);

  const deleteBtn = el("button", { class: "plain-btn danger" });
  deleteBtn.innerHTML = `${icon.trash}<span>Move to recycle bin</span>`;
  const openBtn = el("button", { class: "plain-btn" });
  openBtn.innerHTML = `${icon.external}<span>Open in video player</span>`;
  const folderBtn = el("button", { class: "plain-btn" });
  folderBtn.innerHTML = `${icon.folder}<span>Show in folder</span>`;
  const closeBtn = el("button", { class: "player-close", "aria-label": "Close player", title: "Close (Esc)" });
  closeBtn.innerHTML = icon.close;

  const details = el("dl", { class: "tape-details" },
    el("dt", { text: "Recorded" }), el("dd", { text: dateLabel(tape.startedAt) }),
    el("dt", { text: "Length" }), el("dd", { text: tape.durationSecs ? longDuration(tape.durationSecs) : "Unknown" }),
    el("dt", { text: "Size" }), el("dd", { text: bytes(tape.size) }),
    tape.game ? el("dt", { text: "Category" }) : null, tape.game ? el("dd", { text: tape.game }) : null,
    tape.parts > 1 ? el("dt", { text: "Joined" }) : null, tape.parts > 1 ? el("dd", { text: `${tape.parts} parts after drops` }) : null,
  );

  const player = el("div", { class: "player", role: "dialog", "aria-modal": "true", "aria-label": tape.title },
    el("div", { class: "tv" }, el("div", { class: "tv-bezel" }, screen), el("div", { class: "tv-brand", text: "Rewind" })),
    el("aside", { class: "tape-info" },
      closeBtn,
      el("p", { class: "tape-channel", text: tape.channelName }),
      el("h2", { class: "tape-title", text: tape.title }),
      details,
      el("div", { class: "tape-actions" }, openBtn, folderBtn, deleteBtn),
      el("p", { class: "tape-file", text: tape.fileName }),
    ),
  );
  document.body.append(player);
  requestAnimationFrame(() => player.classList.add("open"));

  let osdTimer = 0;
  const flash = (text: string, stay = false) => {
    osdTop.textContent = text;
    osdTop.classList.add("on");
    clearTimeout(osdTimer);
    if (!stay) osdTimer = window.setTimeout(() => osdTop.classList.remove("on"), 2200);
  };

  if (tape.playable) {
    snow(canvas, 450);
    video.src = fileUrl(tape.path);
    const resume = readResume()[tape.path];
    video.addEventListener("loadedmetadata", () => {
      if (resume && resume > 10 && resume < video.duration - 15) {
        video.currentTime = resume;
        flash(`Resume ${clock(resume)}`);
      }
      void video.play().catch(() => {});
    }, { once: true });
    let lastTime = 0;
    video.addEventListener("play", () => flash("PLAY ▶"));
    video.addEventListener("pause", () => {
      if (!video.ended) flash("PAUSE ❙❙", true);
    });
    video.addEventListener("seeking", () => flash(video.currentTime > lastTime ? "FF ▶▶" : "REW ◀◀"));
    video.addEventListener("ended", () => {
      flash("STOP ■", true);
      saveResume(tape.path, null);
    });
    video.addEventListener("timeupdate", () => {
      lastTime = video.currentTime;
      osdCounter.textContent = clock(video.currentTime);
    });
    video.addEventListener("error", () => {
      flash("NO SIGNAL", true);
      osd("This tape can't play here. Try Open in video player.", "error");
    });
  } else {
    screen.classList.add("no-signal");
    screen.append(
      el("div", { class: "tv-message" },
        el("b", { text: "Raw recording" }),
        el("span", { text: "This tape was saved as a .ts file (ffmpeg was missing or failed). Open it in your video player, VLC plays it." }),
      ),
    );
  }

  const close = () => {
    if (tape.playable && video.currentTime > 10 && !video.ended) saveResume(tape.path, video.currentTime);
    video.pause();
    video.removeAttribute("src");
    video.load();
    sound.eject();
    window.removeEventListener("keydown", onKey, true);
    player.classList.remove("open");
    player.classList.add("closing");
    setTimeout(() => player.remove(), reducedMotion() ? 0 : 260);
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  };
  window.addEventListener("keydown", onKey, true);
  closeBtn.addEventListener("click", close);
  player.addEventListener("pointerdown", (event) => {
    if (event.target === player) close();
  });
  openBtn.addEventListener("click", () => {
    video.pause();
    api.openExternal(tape.path).catch((e) => osd(errorText(e), "error"));
  });
  folderBtn.addEventListener("click", () => api.reveal(tape.path).catch((e) => osd(errorText(e), "error")));
  let armed = false;
  deleteBtn.addEventListener("click", async () => {
    if (!armed) {
      armed = true;
      deleteBtn.classList.add("armed");
      deleteBtn.querySelector("span")!.textContent = "Click again to move to recycle bin";
      return;
    }
    try {
      video.pause();
      video.removeAttribute("src");
      video.load();
      await api.deleteTape(tape.path);
      saveResume(tape.path, null);
      osd("Tape moved to the recycle bin", "info");
      close();
      onChange();
    } catch (e) {
      osd(errorText(e), "error");
    }
  });
  closeBtn.focus();
}
