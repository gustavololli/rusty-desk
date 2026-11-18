#!/usr/bin/env node
// Deterministic native-resolution pixel-art sprite generator for Rusty's yellow robot skin.
// Requirements: Node.js and ImageMagick (`magick`). No AI service, token usage, or packages.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(root, 'assets', 'rusty-16bit');
const framesRoot = path.join(out, 'frames');
const W = 96;
const H = 96;
const FPS = 8;
const states = [
  'Idle', 'Blink', 'LookAround', 'Sleep', 'WakeUp', 'Thinking', 'Listening',
  'Happy', 'Celebrate', 'Focused', 'Error', 'Glitch', 'PomodoroFinished', 'ListeningMusic',
];
const colors = {
  outline: '#753800', deepOutline: '#442000', goldDark: '#aa5707',
  goldShadow: '#ca750a', gold: '#f2a015', goldBright: '#ffc02a',
  goldLight: '#ffdb61', goldGlint: '#fff1ad', navyBlack: '#0d1225',
  navy: '#17213b', navyMid: '#25304d', navyHighlight: '#354462',
  ivory: '#fff7df', white: '#ffffff', cyan: '#3ae8f7', pink: '#e56973',
};
const rgb = (hex) => [1, 3, 5].map(i => Number.parseInt(hex.slice(i, i + 2), 16));
const palette = Object.fromEntries(Object.entries(colors).map(([k, v]) => [k, rgb(v)]));

function canvas() { return new Uint8Array(W * H * 4); }
function pixel(buf, x, y, color, alpha = 255) {
  x = Math.round(x); y = Math.round(y);
  if (x < 0 || y < 0 || x >= W || y >= H) return;
  const i = (y * W + x) * 4;
  const c = palette[color] ?? color;
  buf[i] = c[0]; buf[i + 1] = c[1]; buf[i + 2] = c[2]; buf[i + 3] = alpha;
}
function box(buf, x0, y0, x1, y1, color) {
  for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) pixel(buf, x, y, color);
}
function line(buf, x0, y0, x1, y1, color) {
  const count = Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0), 1);
  for (let i = 0; i <= count; i++) pixel(buf, x0 + (x1 - x0) * i / count, y0 + (y1 - y0) * i / count, color);
}
function disk(buf, cx, cy, rx, ry, color, skew = 0, xBounds) {
  for (let y = Math.floor(cy - ry - 2); y <= Math.ceil(cy + ry + 2); y++) {
    for (let x = Math.floor(cx - rx - 5); x <= Math.ceil(cx + rx + 5); x++) {
      const dx = (x - (cx + (y - cy) * skew)) / rx;
      const dy = (y - cy) / ry;
      if (dx * dx + dy * dy <= 1 && (!xBounds || (x >= xBounds[0] && x <= xBounds[1]))) pixel(buf, x, y, color);
    }
  }
}
function body(buf, yShift) {
  for (let y = 8; y < 89; y++) {
    let inset = 0;
    if (y < 10 || y >= 87) inset = 15;
    else if (y < 12 || y >= 85) inset = 10;
    else if (y < 15 || y >= 82) inset = 6;
    else if (y < 19 || y >= 79) inset = 3;
    const x0 = 7 + inset, x1 = 88 - inset;
    for (let x = x0; x <= x1; x++) {
      let tone = 'gold';
      if (y < 14) tone = 'goldBright';
      if (y < 20 && x < 58) tone = 'goldLight';
      if (x > 78 || y > 78) tone = 'goldShadow';
      if (x > 84 || y > 85) tone = 'goldDark';
      if (x === x0 || x === x1 || y === 8 || y === 88) tone = 'outline';
      if (x >= x0 + 3 && x <= x1 - 3 && y > 19 && y < 77) {
        const h = (x * 17 + y * 29 + x * y * 3) % 137;
        if (h < 8) tone = (x < 55 ? 'goldBright' : 'goldShadow');
      }
      pixel(buf, x, y + yShift, tone);
    }
  }
  // Square-edged 16-bit highlights and an inset bevel around the robot's shell.
  line(buf, 20, 11 + yShift, 73, 11 + yShift, 'goldGlint');
  line(buf, 12, 22 + yShift, 12, 67 + yShift, 'goldLight');
  line(buf, 16, 79 + yShift, 79, 79 + yShift, 'goldDark');
  line(buf, 80, 18 + yShift, 80, 70 + yShift, 'goldShadow');
  box(buf, 19, 13 + yShift, 24, 14 + yShift, 'goldGlint');
  box(buf, 13, 19 + yShift, 14, 24 + yShift, 'goldGlint');
  // Tiny LED panel, kept subdued so the original eyes remain the visual identity.
  box(buf, 76, 17 + yShift, 78, 19 + yShift, 'cyan');
  box(buf, 81, 17 + yShift, 83, 19 + yShift, 'goldLight');
}
function eyes(buf, state, phase, yShift) {
  const cy = 41 + yShift;
  if (state === 'Error' || state === 'Glitch') {
    for (const cx of [34, 63]) {
      line(buf, cx - 12, cy - 13, cx + 12, cy + 13, 'navyBlack');
      line(buf, cx + 12, cy - 13, cx - 12, cy + 13, 'navyBlack');
      line(buf, cx - 11, cy - 13, cx + 13, cy + 13, 'navy');
    }
    return;
  }
  if (state === 'Blink' && (phase === 1 || phase === 2) || state === 'Sleep') {
    for (const cx of [34, 63]) {
      line(buf, cx - 13, cy + 3, cx + 13, cy + 3, 'navyBlack');
      line(buf, cx - 10, cy + 4, cx + 9, cy + 4, 'navy');
    }
    return;
  }
  if (state === 'Happy' || state === 'Celebrate' || state === 'PomodoroFinished') {
    for (const cx of [34, 63]) {
      for (let x = cx - 15; x <= cx + 15; x++) {
        const t = (x - cx) / 15;
        const yy = cy - 7 + Math.round(8 * t * t);
        box(buf, x, yy, x, yy + 3, 'navyBlack');
      }
    }
    return;
  }
  const opening = state === 'Focused' ? 0.63 : state === 'WakeUp' ? 1.08 : 1;
  const look = state === 'LookAround' ? [-2, -1, 2, 1][phase] : 0;
  const lookY = state === 'Thinking' ? -3 : 0;
  const coords = [
    { cx: 34 + look, skew: -0.085, bounds: [16, 48] },
    { cx: 63 + look, skew: 0.075, bounds: [50, 81] },
  ];
  for (const { cx, skew, bounds } of coords) {
    disk(buf, cx, cy + lookY, 15.9, 22.0 * opening, 'navyBlack', skew, bounds);
    disk(buf, cx, cy + lookY, 14.1, 20.1 * opening, 'navy', skew, bounds);
    disk(buf, cx - 3, cy - 10 * opening + lookY, 7.5, 5.5 * opening, 'navyMid', skew, bounds);
    disk(buf, cx - 6, cy - 14 * opening + lookY, 3.1, 2.0 * opening, 'navyHighlight', skew, bounds);
    for (let x = cx - 12; x <= cx + 10; x++) {
      for (let y = cy - 15; y < cy + 14; y++) {
        if ((x * 3 + y * 5) % 13 !== 0) continue;
        const dx = (x - cx - (y - cy) * skew) / 14;
        const dy = (y - cy - lookY) / (20.0 * opening);
        if (dx * dx + dy * dy < 0.85) pixel(buf, x, y, 'navyMid');
      }
    }
  }
}
function mouth(buf, state, phase, yShift) {
  const cx = 49, cy = 73 + yShift;
  const rx = 16, ry = 6;
  // Bright oval speaker border, dark interior, pale vertical bars.
  disk(buf, cx, cy, rx, ry, 'outline');
  disk(buf, cx, cy, rx - 1, ry - 1, 'ivory');
  disk(buf, cx, cy, rx - 3, ry - 2, 'navyBlack');
  for (const x of [40, 44, 48, 52, 56]) {
    const wobble = state === 'Listening' || state === 'ListeningMusic' ? [0, 2, -1, 1][(phase + x) % 4] : 0;
    let top = cy - 3 + Math.max(0, wobble);
    let bot = cy + 3 - Math.max(0, -wobble);
    if (state === 'Sleep') { top = cy; bot = cy + 1; }
    if (state === 'Happy' || state === 'Celebrate' || state === 'PomodoroFinished') { top--; bot++; }
    box(buf, x, top, x + 1, bot, 'ivory');
  }
  pixel(buf, 35, cy - 2, 'goldGlint');
}
function accents(buf, state, phase, yShift) {
  if (state === 'Celebrate' || state === 'PomodoroFinished' || state === 'Happy') {
    for (const [x, y] of [[10, 20], [89, 19], [6, 64], [87, 68]]) {
      if (state === 'Happy' && x > 80) continue;
      line(buf, x - 2, y, x + 2, y, 'ivory');
      line(buf, x, y - 2, x, y + 2, phase % 2 ? 'cyan' : 'goldLight');
    }
  }
  if (state === 'Thinking') {
    for (let n = 0; n < 3; n++) box(buf, 69 + n * 5, 1 + (n === phase % 3 ? 0 : 2), 70 + n * 5, 2 + (n === phase % 3 ? 0 : 2), 'ivory');
  }
  if (state === 'Sleep') {
    box(buf, 72, 6 - phase % 2, 76, 6 - phase % 2, 'ivory');
    line(buf, 76, 6 - phase % 2, 72, 11 - phase % 2, 'ivory');
    box(buf, 72, 11 - phase % 2, 76, 11 - phase % 2, 'ivory');
  }
  if (state === 'ListeningMusic') {
    // Minimal pixel headphones and rhythm notes.
    line(buf, 20, 9 + yShift, 74, 9 + yShift, 'navyMid');
    box(buf, 5, 37 + yShift, 9, 55 + yShift, 'navyMid');
    box(buf, 86, 37 + yShift, 90, 55 + yShift, 'navyMid');
    box(buf, 2, 42 + yShift, 4, 50 + yShift, 'cyan');
    box(buf, 91, 42 + yShift, 93, 50 + yShift, 'cyan');
    const ny = 7 - phase;
    box(buf, 85, ny, 87, ny + 1, 'cyan');
    line(buf, 87, ny, 87, ny + 8, 'cyan');
    box(buf, 84, ny + 8, 87, ny + 9, 'cyan');
  }
  if (state === 'WakeUp' || state === 'PomodoroFinished') {
    box(buf, 88, 8, 90, 17, 'ivory');
    box(buf, 88, 20, 90, 22, 'ivory');
  }
}
function makeFrame(state, phase) {
  const c = canvas();
  const bob = state === 'Celebrate' || state === 'PomodoroFinished' ? [0, -2, -3, -1][phase]
    : state === 'Sleep' ? 1 : state === 'ListeningMusic' ? [0, -1, 0, 1][phase]
      : state === 'Idle' ? [0, -1, 0, 1][phase] : 0;
  body(c, bob);
  eyes(c, state, phase, bob);
  mouth(c, state, phase, bob);
  accents(c, state, phase, bob);
  if (state === 'Glitch' && (phase === 1 || phase === 3)) {
    const yy = 45 + phase * 2;
    const copy = c.slice(yy * W * 4, (yy + 3) * W * 4);
    for (let y = 0; y < 3; y++) for (let x = 0; x < W - 3; x++) {
      const to = ((yy + y) * W + x + 3) * 4, from = (y * W + x) * 4;
      c.set(copy.subarray(from, from + 4), to);
    }
  }
  return c;
}
function writePng(filename, bytes, width = W, height = H) {
  const result = spawnSync('magick', ['-size', `${width}x${height}`, '-depth', '8', 'rgba:-', filename], {
    input: Buffer.from(bytes), maxBuffer: 1024 * 1024,
  });
  if (result.status !== 0) throw new Error(`magick failed for ${filename}: ${result.stderr}`);
}
function paste(target, targetWidth, source, x, y) {
  for (let sy = 0; sy < H; sy++) {
    const from = sy * W * 4;
    const to = ((y + sy) * targetWidth + x) * 4;
    target.set(source.subarray(from, from + W * 4), to);
  }
}

fs.mkdirSync(framesRoot, { recursive: true });
const sheet = new Uint8Array(4 * W * states.length * H * 4);
const manifest = {
  name: 'Rusty 16-bit yellow robot', version: 1, frame_width: W, frame_height: H,
  fps: FPS, columns: 4, rows: states.length, sheet: 'rusty-spritesheet.png',
  format: 'RGBA PNG, nearest-neighbor, transparent background',
  states: {},
};
for (const [row, state] of states.entries()) {
  const name = state.replace(/[A-Z]/g, (char, index) => index ? `_${char.toLowerCase()}` : char.toLowerCase());
  const folder = path.join(framesRoot, name);
  fs.mkdirSync(folder, { recursive: true });
  const files = [];
  for (let i = 0; i < 4; i++) {
    const frame = makeFrame(state, i);
    const rel = `frames/${name}/${String(i).padStart(2, '0')}.png`;
    writePng(path.join(out, rel), frame);
    paste(sheet, 4 * W, frame, i * W, row * H);
    files.push(rel);
  }
  manifest.states[state] = { row, frames: files, frame_duration_ms: 1000 / FPS };
}
writePng(path.join(out, 'rusty-spritesheet.png'), sheet, 4 * W, states.length * H);
const master = makeFrame('Idle', 0);
writePng(path.join(out, 'rusty-idle.png'), master);
fs.writeFileSync(path.join(out, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);

// GIF is only a quick human preview; application should consume the lossless PNGs.
const previewFrames = ['Idle', 'Blink', 'LookAround', 'Thinking', 'Happy', 'Celebrate', 'Sleep', 'WakeUp', 'ListeningMusic'];
const preview = previewFrames.flatMap(state => manifest.states[state].frames.map(rel => path.join(out, rel)));
const gif = spawnSync('magick', ['-delay', '12', ...preview, '-loop', '0', path.join(out, 'preview.gif')], { maxBuffer: 1024 * 1024 });
if (gif.status !== 0) throw new Error(`preview gif failed: ${gif.stderr}`);
console.log(`Generated ${states.length * 4} PNG frames, sprite sheet, idle and preview in ${out}`);
