// Hand-drawn 16px icons, stroke = currentColor.
const wrap = (body: string) =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

export const icon = {
  rec: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="5" fill="currentColor"/></svg>`,
  stop: `<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3.5" y="3.5" width="9" height="9" rx="1.5" fill="currentColor"/></svg>`,
  play: `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M5 3.2v9.6c0 .5.5.8.9.5l7-4.8a.6.6 0 000-1L5.9 2.7c-.4-.3-.9 0-.9.5z" fill="currentColor"/></svg>`,
  pause: wrap(`<path d="M5.5 3.5v9M10.5 3.5v9"/>`),
  plus: wrap(`<path d="M8 3v10M3 8h10"/>`),
  close: wrap(`<path d="M4 4l8 8M12 4l-8 8"/>`),
  folder: wrap(`<path d="M2 4.5c0-.6.4-1 1-1h3l1.5 1.5H13c.6 0 1 .4 1 1v6c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z"/>`),
  trash: wrap(`<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.3c0 .4.4.7.8.7h4.2c.4 0 .8-.3.8-.7l.6-8.3"/>`),
  external: wrap(`<path d="M9 3h4v4M13 3L7.5 8.5M11 9.5V13H3V5h3.5"/>`),
  refresh: wrap(`<path d="M13 8a5 5 0 11-1.5-3.6M13 2.5v3h-3"/>`),
  eye: wrap(`<path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>`),
  eyeOff: wrap(`<path d="M2 2l12 12M6.5 4c.5-.2 1-.3 1.5-.3 4 0 6.5 4.3 6.5 4.3s-.6 1.1-1.8 2.3M4 5.3C2.4 6.5 1.5 8 1.5 8S4 12.5 8 12.5c1 0 1.9-.3 2.7-.7"/>`),
  dots: `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="3.5" cy="8" r="1.4" fill="currentColor"/><circle cx="8" cy="8" r="1.4" fill="currentColor"/><circle cx="12.5" cy="8" r="1.4" fill="currentColor"/></svg>`,
  search: wrap(`<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14"/>`),
  twitch: `<svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M3 1.5L1.8 4.3v9h3V15h1.8l1.7-1.7h2.6l3.3-3.3V1.5zm9.6 7.9l-1.9 1.9H7.8l-1.7 1.7v-1.7H3.6V2.7h9z"/><path fill="currentColor" d="M10.4 4.6h1.2v3.5h-1.2zM7.3 4.6h1.2v3.5H7.3z"/></svg>`,
  youtube: `<svg viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" d="M15.3 4.3a2 2 0 00-1.4-1.4C12.7 2.6 8 2.6 8 2.6s-4.7 0-5.9.3A2 2 0 00.7 4.3C.4 5.5.4 8 .4 8s0 2.5.3 3.7a2 2 0 001.4 1.4c1.2.3 5.9.3 5.9.3s4.7 0 5.9-.3a2 2 0 001.4-1.4c.3-1.2.3-3.7.3-3.7s0-2.5-.3-3.7zM6.5 10.3V5.7L10.4 8z"/></svg>`,
};
