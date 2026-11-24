// A 7-segment VFD clock drawn in SVG. Unlit segments stay faintly visible, like real glass.

const SEGMENTS: Record<string, string> = {
  a: "9,2 31,2 34,5 31,8 9,8 6,5",
  b: "35,6 38,9 38,31 35,34 32,31 32,9",
  c: "35,36 38,39 38,61 35,64 32,61 32,39",
  d: "9,62 31,62 34,65 31,68 9,68 6,65",
  e: "5,36 8,39 8,61 5,64 2,61 2,39",
  f: "5,6 8,9 8,31 5,34 2,31 2,9",
  g: "9,32 31,32 34,35 31,38 9,38 6,35",
};

const GLYPHS: Record<string, string> = {
  "0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc",
  "5": "afgcd", "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg",
  "-": "g", " ": "",
};

function digit(): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const root = document.createElementNS(ns, "svg");
  root.setAttribute("viewBox", "0 0 40 70");
  root.classList.add("seg-digit");
  for (const [name, points] of Object.entries(SEGMENTS)) {
    const poly = document.createElementNS(ns, "polygon");
    poly.setAttribute("points", points);
    poly.dataset.seg = name;
    root.append(poly);
  }
  return root;
}

export class SegmentClock {
  private digits: SVGSVGElement[] = [];
  private colon: HTMLElement;

  constructor(host: HTMLElement) {
    host.textContent = "";
    for (let i = 0; i < 4; i++) {
      const d = digit();
      this.digits.push(d);
      host.append(d);
      if (i === 1) {
        this.colon = document.createElement("span");
        this.colon.className = "seg-colon";
        this.colon.innerHTML = "<i></i><i></i>";
        host.append(this.colon);
      }
    }
    this.colon ??= document.createElement("span");
  }

  /** text = 4 characters, e.g. "1200" or "--  ". */
  show(text: string) {
    const chars = text.padStart(4, " ").slice(-4).split("");
    chars.forEach((ch, i) => {
      const lit = GLYPHS[ch] ?? "";
      for (const poly of this.digits[i].querySelectorAll<SVGPolygonElement>("polygon")) {
        poly.classList.toggle("on", lit.includes(poly.dataset.seg ?? "?"));
      }
    });
  }

  colonOn(on: boolean) {
    this.colon.classList.toggle("on", on);
  }

  blink(on: boolean) {
    this.digits.forEach((d) => d.parentElement?.classList.toggle("blinking", on));
  }
}
