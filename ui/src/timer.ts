// Timer programs, like programming a VCR: days + from/to. Shown as a blue on-screen menu.

import { api, errorText, type ChannelView, type Timer } from "./api";
import { icon } from "./icons";
import { osd } from "./osd";
import * as sound from "./sound";
import { el } from "./util";

const DAY_SHORT = ["M", "T", "W", "T", "F", "S", "S"];
const DAY_NAME = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const EVERY_DAY = 0x7f;
const WEEKDAYS = 0x1f;
const WEEKEND = 0x60;
const MAX_PROGRAMS = 8;
export const LEAD_MINUTES = 10;

const hhmm = (minutes: number) => `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
const fromInput = (value: string) => {
  const [h, m] = value.split(":").map(Number);
  return Number.isFinite(h) && Number.isFinite(m) ? (h * 60 + m) % 1440 : 0;
};

export function daysLabel(days: number): string {
  if ((days & EVERY_DAY) === EVERY_DAY) return "Every day";
  if ((days & EVERY_DAY) === WEEKDAYS) return "Weekdays";
  if ((days & EVERY_DAY) === WEEKEND) return "Weekends";
  return DAY_NAME.filter((_, i) => days & (1 << i)).join(", ");
}

/** "Fri 21:00-01:00", "+1 more" when there are several programs. */
export function timerSummary(timers: Timer[]): string {
  const on = timers.filter((t) => t.enabled && t.days & EVERY_DAY);
  if (!on.length) return "";
  const first = on[0];
  const text = `${daysLabel(first.days)} ${hhmm(first.start)}–${hhmm(first.end)}`;
  return on.length > 1 ? `${text}, +${on.length - 1} more` : text;
}

/** "Fri 21:00" or "Today 21:00" for a timestamp. */
export function whenLabel(ms: number): string {
  const d = new Date(ms);
  const now = new Date();
  const tomorrow = new Date();
  tomorrow.setDate(now.getDate() + 1);
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === now.toDateString()) return `today ${time}`;
  if (d.toDateString() === tomorrow.toDateString()) return `tomorrow ${time}`;
  return `${d.toLocaleDateString(undefined, { weekday: "short" })} ${time}`;
}

export function openTimerEditor(channel: ChannelView) {
  document.querySelector(".timer-modal")?.remove();
  const programs: Timer[] = channel.timers.length
    ? channel.timers.map((t) => ({ ...t }))
    : [{ days: EVERY_DAY, start: 21 * 60, end: 1 * 60, enabled: true }];

  const list = el("div", { class: "timer-list" });
  const addBtn = el("button", { class: "osd-btn", type: "button" });
  addBtn.innerHTML = `${icon.plus}<span>Add program</span>`;
  const save = el("button", { class: "osd-btn strong", type: "button", text: "Save timer" });
  const cancel = el("button", { class: "osd-btn", type: "button", text: "Cancel" });
  const clear = el("button", { class: "osd-btn ghost", type: "button", text: "Remove timer, watch all the time" });

  const box = el("div", { class: "timer-box", role: "dialog", "aria-modal": "true", "aria-label": `Timer for ${channel.name}` },
    el("div", { class: "osd-head" },
      el("h2", { text: "Timer program" }),
      el("span", { class: "timer-channel", text: channel.name }),
    ),
    el("p", { class: "timer-help", text: `Rewind looks for ${channel.name} only at these times, starting ${LEAD_MINUTES} minutes early. A stream that starts in time is recorded to the end, even after the "to" time.` }),
    list,
    el("div", { class: "timer-add" }, addBtn),
    el("div", { class: "timer-foot" }, clear, el("span", { class: "timer-gap" }), cancel, save),
  );
  const modal = el("div", { class: "timer-modal" }, box);

  const render = () => {
    list.replaceChildren(...programs.map((p, index) => programRow(p, index)));
    addBtn.disabled = programs.length >= MAX_PROGRAMS;
    clear.hidden = channel.timers.length === 0;
  };

  const programRow = (p: Timer, index: number): HTMLElement => {
    const days = el("div", { class: "timer-days", role: "group", "aria-label": "Days" });
    DAY_SHORT.forEach((letter, i) => {
      const on = Boolean(p.days & (1 << i));
      const b = el("button", { class: "day", type: "button", "aria-pressed": String(on), title: DAY_NAME[i], text: letter });
      b.addEventListener("click", () => {
        sound.click();
        p.days ^= 1 << i;
        render();
      });
      days.append(b);
    });
    const presets = el("div", { class: "timer-presets" },
      ...[["Every day", EVERY_DAY], ["Weekdays", WEEKDAYS], ["Weekends", WEEKEND]].map(([label, mask]) => {
        const b = el("button", { class: "preset", type: "button", text: String(label), "aria-pressed": String((p.days & EVERY_DAY) === mask) });
        b.addEventListener("click", () => {
          sound.click();
          p.days = Number(mask);
          render();
        });
        return b;
      }),
    );
    const from = el("input", { class: "time", type: "time", value: hhmm(p.start), step: "300", "aria-label": "From" });
    const to = el("input", { class: "time", type: "time", value: hhmm(p.end), step: "300", "aria-label": "To" });
    from.addEventListener("change", () => {
      p.start = fromInput(from.value);
      render();
    });
    to.addEventListener("change", () => {
      p.end = fromInput(to.value);
      render();
    });
    const onOff = el("button", { class: "osd-btn small", type: "button", "aria-pressed": String(p.enabled), text: p.enabled ? "On" : "Off", title: "Turn this program on or off" });
    onOff.addEventListener("click", () => {
      sound.click();
      p.enabled = !p.enabled;
      render();
    });
    const remove = el("button", { class: "timer-remove", type: "button", "aria-label": `Remove program ${index + 1}`, title: "Remove program" });
    remove.innerHTML = icon.close;
    remove.addEventListener("click", () => {
      sound.click();
      programs.splice(index, 1);
      render();
    });

    const overnight = p.end <= p.start;
    const length = ((p.end - p.start + 1440) % 1440) || 1440;
    const hint = !(p.days & EVERY_DAY)
      ? "Pick at least one day"
      : `${daysLabel(p.days)}, looks from ${hhmm((p.start - LEAD_MINUTES + 1440) % 1440)} for ${Math.floor(length / 60)}h${length % 60 ? ` ${length % 60}m` : ""}${overnight && p.end !== p.start ? ", ends the next day" : ""}`;

    return el("div", { class: `timer-program${p.enabled ? "" : " off"}` },
      el("span", { class: "prog-no", text: `P${index + 1}` }),
      el("div", { class: "prog-body" },
        el("div", { class: "prog-line" }, days, presets),
        el("div", { class: "prog-line" },
          el("label", { class: "time-field" }, el("span", { text: "From" }), from),
          el("label", { class: "time-field" }, el("span", { text: "To" }), to),
          onOff,
        ),
        el("span", { class: `prog-hint${p.days & EVERY_DAY ? "" : " bad"}`, text: hint }),
      ),
      remove,
    );
  };

  const close = () => {
    window.removeEventListener("keydown", onKey, true);
    modal.classList.add("closing");
    setTimeout(() => modal.remove(), 180);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };
  const commit = async (next: Timer[]) => {
    if (next.some((p) => !(p.days & EVERY_DAY))) {
      osd("Pick at least one day for each program", "error");
      return;
    }
    save.disabled = true;
    try {
      await api.setTimers(channel.id, next);
      sound.insert();
      osd(next.length ? `Timer set for ${channel.name}: ${timerSummary(next) || "all programs off"}` : `${channel.name} is watched all the time again`, "info", 5000);
      close();
    } catch (e) {
      osd(errorText(e), "error");
      save.disabled = false;
    }
  };

  addBtn.addEventListener("click", () => {
    sound.click();
    const last = programs[programs.length - 1];
    programs.push(last ? { ...last } : { days: EVERY_DAY, start: 21 * 60, end: 1 * 60, enabled: true });
    render();
  });
  save.addEventListener("click", () => commit(programs));
  cancel.addEventListener("click", close);
  clear.addEventListener("click", () => commit([]));
  modal.addEventListener("pointerdown", (e) => {
    if (e.target === modal) close();
  });
  window.addEventListener("keydown", onKey, true);

  render();
  document.body.append(modal);
  save.focus();
}
