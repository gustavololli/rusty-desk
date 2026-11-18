// Procedural Rusty at a 96x96 logical-pixel resolution: no sprites or images.
// The head, each eye and the grille are separate animated layers.
// The browser scales the tiny canvas using nearest-neighbor CSS.

const W = 96;
const H = 96;
const C = Object.freeze({
  outline: '#753800', dark: '#442000', side: '#ca750a', shadow: '#aa5707',
  yellow: '#f2a015', bright: '#ffc02a', light: '#ffdb61', glint: '#fff1ad',
  black: '#0d1225', navy: '#17213b', blue: '#25304d', blueLight: '#354462',
  ivory: '#fff7df', cyan: '#3ae8f7', red: '#e56973',
});

function rect(ctx, x, y, w, h, color) {
  ctx.fillStyle = color;
  ctx.fillRect(Math.round(x), Math.round(y), Math.max(1, Math.round(w)), Math.max(1, Math.round(h)));
}

function oval(ctx, cx, cy, rx, ry, color, skew = 0, bounds = [0, 95]) {
  if (rx <= 0 || ry <= 0) return;
  ctx.fillStyle = color;
  for (let y = Math.floor(cy - ry - 1); y <= Math.ceil(cy + ry + 1); y++) {
    const dy = (y - cy) / ry;
    if (Math.abs(dy) > 1) continue;
    const half = rx * Math.sqrt(1 - dy * dy);
    const rowCx = cx + (y - cy) * skew;
    const first = Math.max(bounds[0], Math.ceil(rowCx - half));
    const last = Math.min(bounds[1], Math.floor(rowCx + half));
    if (first <= last) ctx.fillRect(first, y, last - first + 1, 1);
  }
}

function drawBody(ctx) {
  for (let y = 8; y < 89; y++) {
    let inset = 0;
    if (y < 10 || y >= 87) inset = 15;
    else if (y < 12 || y >= 85) inset = 10;
    else if (y < 15 || y >= 82) inset = 6;
    else if (y < 19 || y >= 79) inset = 3;
    const x0 = 7 + inset;
    const x1 = 88 - inset;
    for (let x = x0; x <= x1; x++) {
      let color = C.yellow;
      if (y < 14) color = C.bright;
      if (y < 20 && x < 58) color = C.light;
      if (x > 78 || y > 78) color = C.side;
      if (x > 84 || y > 85) color = C.shadow;
      if (x === x0 || x === x1 || y === 8 || y === 88) color = C.outline;
      if (x >= x0 + 3 && x <= x1 - 3 && y > 19 && y < 77) {
        if ((x * 17 + y * 29 + x * y * 3) % 137 < 8) {
          color = x < 55 ? C.bright : C.side;
        }
      }
      rect(ctx, x, y, 1, 1, color);
    }
  }
  rect(ctx, 20, 11, 53, 1, C.glint);
  rect(ctx, 12, 22, 1, 46, C.light);
  rect(ctx, 16, 79, 64, 1, C.shadow);
  rect(ctx, 80, 18, 1, 53, C.side);
  rect(ctx, 19, 13, 6, 2, C.glint);
  rect(ctx, 13, 19, 2, 6, C.glint);
  rect(ctx, 76, 17, 3, 3, C.cyan);
  rect(ctx, 81, 17, 3, 3, C.light);
}

function eye(ctx, cx, cy, dx, dy, open, skew, bounds) {
  const x = cx + dx;
  const y = cy + dy;
  const ry = Math.max(0.75, 22 * open);
  oval(ctx, x, y, 15.9, ry, C.black, skew, bounds);
  if (open > 0.09) {
    oval(ctx, x, y, 14.2, Math.max(0.9, 20 * open), C.navy, skew, bounds);
    oval(ctx, x - 3, y - 10 * open, 7.5, Math.max(0.6, 5.6 * open), C.blue, skew, bounds);
    oval(ctx, x - 6, y - 14 * open, 3.1, Math.max(0.5, 2 * open), C.blueLight, skew, bounds);
  } else {
    rect(ctx, x - 13, y, 26, 1, C.navy);
  }
}

function grille(ctx, open) {
  const cx = 49;
  const cy = 72.5;
  const radiusY = 5.9 + open * 1.6;
  oval(ctx, cx, cy, 16.5, radiusY + 1, C.outline);
  oval(ctx, cx, cy, 15, radiusY, C.ivory);
  oval(ctx, cx, cy, 13.2, Math.max(2.8, radiusY - 1.8), C.black);
  for (const x of [40, 44, 48, 52, 56]) {
    const y = Math.round(cy - (2.8 + open));
    rect(ctx, x, y, 2, 7 + open * 2, C.ivory);
  }
}

function particles(ctx, pose) {
  const flicker = Math.floor(pose.time * 9) % 2;
  if (pose.state === 'happy') {
    for (const [x, y] of [[8, 22], [87, 27], [89, 71]]) {
      rect(ctx, x, y - (flicker ? 1 : 0), 2, 6, C.glint);
      rect(ctx, x - 2, y + 2 - (flicker ? 1 : 0), 6, 2, C.cyan);
    }
  }
  if (pose.state === 'thinking') {
    for (let i = 0; i < 3; i++) {
      const y = 4 - (Math.floor(pose.time * 3) + i) % 3;
      rect(ctx, 72 + i * 6, y, 3, 3, C.ivory);
    }
  }
  if (pose.state === 'sleep') {
    rect(ctx, 77, 5 + flicker, 6, 2, C.ivory);
    rect(ctx, 80, 7 + flicker, 2, 3, C.ivory);
    rect(ctx, 77, 10 + flicker, 6, 2, C.ivory);
  }
  if (pose.state === 'error') {
    rect(ctx, 76, 17, 3, 3, C.red);
    rect(ctx, 88, 5, 3, 9, C.red);
    rect(ctx, 88, 16, 3, 3, C.red);
  }
}

export function createRustyRenderer(canvas) {
  if (!(canvas instanceof HTMLCanvasElement)) throw new TypeError('A canvas is required');
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext('2d', { alpha: true });
  ctx.imageSmoothingEnabled = false;
  // Cache only the *procedurally drawn* shell, never the pre-existing sprite assets.
  const shell = document.createElement('canvas');
  shell.width = W;
  shell.height = H;
  drawBody(shell.getContext('2d'));

  return function render(pose) {
    ctx.clearRect(0, 0, W, H);
    ctx.save();
    ctx.translate(48 + pose.headX, 48 + pose.headY);
    ctx.rotate(pose.tilt * Math.PI / 180);
    ctx.scale(pose.scaleX, pose.scaleY);
    ctx.translate(-48, -48);
    ctx.drawImage(shell, 0, 0);
    // Eye seats are independent springs. Default skew makes them slightly cross-eyed.
    eye(ctx, 34, 41, pose.leftX, pose.leftY, pose.eyeOpen, -0.085, [16, 48]);
    eye(ctx, 63, 41, pose.rightX, pose.rightY, pose.eyeOpen, 0.075, [50, 81]);
    grille(ctx, pose.mouthOpen);
    particles(ctx, pose);
    ctx.restore();
  };
}
