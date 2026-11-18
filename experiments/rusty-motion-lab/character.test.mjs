import test from 'node:test';
import assert from 'node:assert/strict';
import { RustyMotion } from './motion.mjs';
import { createRustyRenderer } from './character.mjs';

test('procedural character draws all animation states without sprite files', () => {
  const operations = [];
  class FakeCanvas {
    constructor() {
      this.context = {
        fillStyle: '', imageSmoothingEnabled: true,
        fillRect: (...args) => operations.push(['fillRect', ...args]),
        clearRect: (...args) => operations.push(['clearRect', ...args]),
        save: () => operations.push(['save']),
        restore: () => operations.push(['restore']),
        translate: (...args) => operations.push(['translate', ...args]),
        rotate: (...args) => operations.push(['rotate', ...args]),
        scale: (...args) => operations.push(['scale', ...args]),
        drawImage: (...args) => operations.push(['drawImage', ...args]),
      };
    }
    getContext() { return this.context; }
  }
  const oldDocument = globalThis.document;
  const oldCanvasClass = globalThis.HTMLCanvasElement;
  globalThis.HTMLCanvasElement = FakeCanvas;
  globalThis.document = { createElement: () => new FakeCanvas() };
  try {
    const target = new FakeCanvas();
    const draw = createRustyRenderer(target);
    const motion = new RustyMotion();
    for (const state of ['idle', 'thinking', 'listening', 'focused', 'happy', 'error', 'sleep']) {
      motion.setState(state);
      draw(motion.update(0.3));
    }
    assert.equal(target.width, 96);
    assert.equal(target.height, 96);
    assert.equal(target.context.imageSmoothingEnabled, false);
    assert.equal(operations.filter(([op]) => op === 'drawImage').length, 7);
    assert.ok(operations.filter(([op]) => op === 'fillRect').length > 1000);
    assert.equal(operations.filter(([op]) => op === 'save').length, 7);
    assert.equal(operations.filter(([op]) => op === 'restore').length, 7);
  } finally {
    globalThis.document = oldDocument;
    globalThis.HTMLCanvasElement = oldCanvasClass;
  }
});
