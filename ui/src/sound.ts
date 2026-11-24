// Tiny synthesized mechanical sounds (no audio files). Off when Settings > sounds is off.

let ctx: AudioContext | null = null;
let enabled = true;

export function setSounds(on: boolean) {
  enabled = on;
}

function audio(): AudioContext | null {
  if (!enabled) return null;
  try {
    ctx ??= new AudioContext();
    if (ctx.state === "suspended") void ctx.resume();
    return ctx;
  } catch {
    return null;
  }
}

function noiseBurst(ac: AudioContext, at: number, length: number, freq: number, gain: number) {
  const buffer = ac.createBuffer(1, Math.ceil(ac.sampleRate * length), ac.sampleRate);
  const data = buffer.getChannelData(0);
  for (let i = 0; i < data.length; i++) data[i] = (Math.random() * 2 - 1) * (1 - i / data.length) ** 3;
  const src = ac.createBufferSource();
  src.buffer = buffer;
  const filter = ac.createBiquadFilter();
  filter.type = "bandpass";
  filter.frequency.value = freq;
  filter.Q.value = 1.2;
  const g = ac.createGain();
  g.gain.value = gain;
  src.connect(filter).connect(g).connect(ac.destination);
  src.start(at);
}

function thump(ac: AudioContext, at: number, freq: number, gain: number, length = 0.08) {
  const osc = ac.createOscillator();
  const g = ac.createGain();
  osc.type = "sine";
  osc.frequency.setValueAtTime(freq, at);
  osc.frequency.exponentialRampToValueAtTime(freq * 0.5, at + length);
  g.gain.setValueAtTime(gain, at);
  g.gain.exponentialRampToValueAtTime(0.0001, at + length);
  osc.connect(g).connect(ac.destination);
  osc.start(at);
  osc.stop(at + length + 0.02);
}

/** A piano-key VCR button. */
export function click() {
  const ac = audio();
  if (!ac) return;
  const t = ac.currentTime;
  noiseBurst(ac, t, 0.025, 3200, 0.18);
  thump(ac, t, 180, 0.12, 0.05);
}

/** A cassette being pushed into the slot. */
export function insert() {
  const ac = audio();
  if (!ac) return;
  const t = ac.currentTime;
  noiseBurst(ac, t, 0.12, 900, 0.12);
  thump(ac, t + 0.1, 110, 0.25, 0.12);
  noiseBurst(ac, t + 0.12, 0.03, 2600, 0.2);
}

/** Eject: spring + clack. */
export function eject() {
  const ac = audio();
  if (!ac) return;
  const t = ac.currentTime;
  noiseBurst(ac, t, 0.04, 2400, 0.2);
  thump(ac, t + 0.02, 240, 0.15, 0.09);
}
