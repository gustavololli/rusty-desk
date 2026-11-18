import test from 'node:test';
import assert from 'node:assert/strict';
import { RustyMotion, Spring, STATES } from './motion.mjs';

const advance = (motion, seconds, dt = 1 / 120) => {
  let pose = motion.pose();
  const count = Math.round(seconds / dt);
  for (let i = 0; i < count; i++) pose = motion.update(dt);
  return pose;
};

test('damped spring converges without teleporting', () => {
  const spring = new Spring(0, 3.8, 0.82);
  spring.step(10, 1 / 120);
  assert.ok(spring.value > 0 && spring.value < 10);
  for (let i = 0; i < 300; i++) spring.step(10, 1 / 120);
  assert.ok(Math.abs(spring.value - 10) < 0.01);
  assert.ok(Math.abs(spring.velocity) < 0.05);
});

test('the timeline stays deterministic under the same seed', () => {
  const first = new RustyMotion({ seed: 1212 });
  const second = new RustyMotion({ seed: 1212 });
  for (let i = 0; i < 1800; i++) {
    assert.deepEqual(first.update(1 / 120), second.update(1 / 120));
  }
});

test('all states have finite poses and smooth head transitions', () => {
  for (const state of STATES) {
    const motion = new RustyMotion();
    motion.setState(state);
    const first = motion.update(1 / 120);
    const settled = advance(motion, 1.2);
    for (const pose of [first, settled]) {
      for (const [key, value] of Object.entries(pose)) {
        if (typeof value === 'number') assert.ok(Number.isFinite(value), `${state}: ${key}`);
      }
      assert.ok(pose.eyeOpen > 0 && pose.eyeOpen <= 1.16);
    }
    if (state === 'thinking') {
      assert.ok(Math.abs(first.tilt) < Math.abs(settled.tilt));
      assert.ok(settled.leftY < -1);
    }
    if (state === 'sleep') assert.ok(settled.eyeOpen < 0.2);
  }
  assert.throws(() => new RustyMotion().setState('not-a-state'));
});

test('the left and right eyes move independently when following the pointer', () => {
  const motion = new RustyMotion();
  motion.setPointer(1, -1);
  const pose = advance(motion, 0.5);
  assert.ok(pose.leftX > 2);
  assert.ok(pose.rightX > pose.leftX);
  assert.ok(pose.leftY < -1.7);
  assert.ok(pose.rightY > pose.leftY);
  assert.ok(pose.tilt > 1);
});

test('idle has restrained motion and a non-looping glance schedule', () => {
  const motion = new RustyMotion({ seed: 982 });
  const early = advance(motion, 0.5);
  assert.ok(Math.abs(early.headY) <= 1);
  assert.ok(Math.abs(early.tilt) <= 1.2);
  const later = advance(motion, 3.3);
  assert.ok(Math.abs(later.leftX) > 0.03 || Math.abs(later.leftY) > 0.03);
});

test('blink opens again and does not leave the face closed', () => {
  const motion = new RustyMotion();
  let minimum = 1;
  // Allow enough time for the occasional intentional double blink.
  for (let i = 0; i < 480; i++) {
    const pose = motion.update(1 / 120);
    minimum = Math.min(minimum, pose.eyeOpen);
  }
  assert.ok(minimum < 0.2, `minimum open factor: ${minimum}`);
  assert.ok(motion.pose().eyeOpen > 0.98);
});

test('reduced motion has a stable head and no blinks', () => {
  const motion = new RustyMotion({ reducedMotion: true });
  motion.setPointer(1, -1);
  motion.setState('thinking');
  let pose = advance(motion, 8);
  assert.equal(pose.headX, 0);
  assert.equal(pose.headY, 0);
  assert.equal(pose.tilt, 0);
  assert.ok(pose.eyeOpen > 0.9);
  motion.setState('sleep');
  pose = advance(motion, 0.1);
  assert.ok(pose.eyeOpen < 0.2); // expressive still, no movement
});

test('invalid and huge frame durations are bounded safely', () => {
  const motion = new RustyMotion();
  assert.equal(motion.update(Number.NaN).time, 0);
  assert.equal(motion.update(-10).time, 0);
  const pose = motion.update(120);
  assert.ok(pose.time <= 0.1 + 1e-8);
});

test('a distraction moves eyes first and head follows, then both recover', () => {
  const motion = new RustyMotion({ seed: 42 });
  motion.setState('focused'); // Suppress idle's unrelated spontaneous glance.
  motion.noticeSomething('right');
  const early = advance(motion, 0.11);
  assert.ok(early.leftX > 0.25);
  assert.ok(early.tilt > 0);
  const looking = advance(motion, 0.7);
  assert.ok(looking.leftX > 2);
  assert.ok(looking.tilt > 3);
  const recovered = advance(motion, 2);
  assert.equal(motion.cue, null);
  assert.ok(Math.abs(recovered.leftX) < 0.3);
  assert.ok(Math.abs(recovered.tilt - 1.2) < 0.3);
});

test('thinking inclines the head while eyes can move on their own', () => {
  const motion = new RustyMotion();
  motion.setState('thinking');
  const pose = advance(motion, 1);
  assert.ok(pose.tilt < -3);
  assert.ok(pose.leftY < -1.5);
});

test('user and model writing look down, and stop when writing ends', () => {
  const motion = new RustyMotion();
  motion.setPointer(0, -1);
  motion.setTyping('user', true);
  let pose = advance(motion, 0.7);
  assert.ok(pose.leftY > 1.8);
  assert.ok(pose.headY > 1);
  motion.setTyping('user', false);
  motion.setTyping('model', true);
  pose = advance(motion, 0.5);
  assert.ok(pose.rightY > 1.6);
  motion.setTyping('model', false);
  pose = advance(motion, 0.8);
  assert.ok(pose.leftY < 0);
  assert.throws(() => motion.setTyping('unknown', true));
});

test('before responding Rusty faces the person instead of the cursor', () => {
  const motion = new RustyMotion();
  motion.setPointer(-1, 0.8);
  advance(motion, 0.4);
  motion.prepareResponse();
  const pose = advance(motion, 0.65);
  assert.ok(Math.abs(pose.leftX) < 0.3);
  assert.ok(pose.headY < -0.1);
  advance(motion, 1.1);
  assert.equal(motion.cue, null);
});

test('reduced motion keeps the new choreography free of head movement', () => {
  const motion = new RustyMotion({ reducedMotion: true });
  motion.noticeSomething('left');
  motion.setTyping('user', true);
  const pose = advance(motion, 0.8);
  assert.equal(pose.tilt, 0);
  assert.equal(pose.headY, 0);
  assert.ok(pose.leftY > 1);
});
