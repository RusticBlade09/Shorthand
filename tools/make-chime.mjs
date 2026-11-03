// Generates src-tauri/sounds/chime.wav — Shorthand's own "//" sound, so it can't be
// mistaken for a Windows notification. Two soft rising bell notes (G5 → D6).
// Run: node tools/make-chime.mjs
import { writeFileSync, mkdirSync } from 'node:fs';

const rate = 44100, seconds = 0.6, peak = 0.35;
const notes = [{ hz: 784, at: 0 }, { hz: 1175, at: 0.09 }];
const n = Math.round(rate * seconds);
const samples = new Float32Array(n);

for (const { hz, at } of notes) {
  for (let i = Math.round(at * rate); i < n; i++) {
    const t = i / rate - at;
    const env = Math.min(1, t / 0.005) * Math.exp(-t / 0.16); // 5 ms attack, soft decay
    const tone = Math.sin(2 * Math.PI * hz * t) + 0.25 * Math.sin(2 * Math.PI * hz * 2 * t);
    samples[i] += env * tone;
  }
}

const max = samples.reduce((m, s) => Math.max(m, Math.abs(s)), 0);
const data = Buffer.alloc(n * 2);
samples.forEach((s, i) => data.writeInt16LE(Math.round((s / max) * peak * 32767), i * 2));

const header = Buffer.alloc(44);
header.write('RIFF', 0); header.writeUInt32LE(36 + data.length, 4); header.write('WAVE', 8);
header.write('fmt ', 12); header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); // PCM
header.writeUInt16LE(1, 22); header.writeUInt32LE(rate, 24); header.writeUInt32LE(rate * 2, 28); // mono
header.writeUInt16LE(2, 32); header.writeUInt16LE(16, 34); // 16-bit
header.write('data', 36); header.writeUInt32LE(data.length, 40);

const out = new URL('../src-tauri/sounds/chime.wav', import.meta.url);
mkdirSync(new URL('.', out), { recursive: true });
writeFileSync(out, Buffer.concat([header, data]));
console.log('wrote', out.pathname, header.length + data.length, 'bytes');
