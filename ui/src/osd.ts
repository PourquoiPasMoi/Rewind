// Notifications. Like a real VCR, messages appear on the deck's own display (deck.ts listens
// for "rewind-flash"), so nothing covers the app. Problems also get a small card in the
// bottom-right corner, because those need attention.

import { el } from "./util";

export type OsdKind = "rec" | "saved" | "error" | "info";

export function osd(text: string, kind: OsdKind = "info", ms = 4200) {
  window.dispatchEvent(new CustomEvent("rewind-flash", { detail: { text, kind, ms } }));
  if (kind !== "error") return;
  const host = document.getElementById("osd");
  if (!host) return;
  const prefix = { rec: "REC", saved: "SAVED", error: "CHECK", info: "" }[kind];
  const item = el("div", { class: `osd osd-${kind}`, role: kind === "error" ? "alert" : "status" },
    prefix ? el("b", { text: prefix }) : null,
    el("span", { text }),
  );
  host.prepend(item);
  while (host.children.length > 2) host.lastElementChild?.remove();
  setTimeout(() => {
    item.classList.add("out");
    item.addEventListener("animationend", () => item.remove(), { once: true });
    setTimeout(() => item.remove(), 600);
  }, ms);
}
