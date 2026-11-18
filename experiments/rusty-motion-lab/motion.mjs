// A small, original motion experiment inspired by the techniques described for
// Grok Bot (state choreography, damped springs and living gaze). No sprite assets.
// Pure JS: portable to Rust, deterministic under a fixed seed and timestep.

export const STATES = Object.freeze([
  'idle', 'thinking', 'listening', 'focused', 'happy', 'error', 'sleep',
]);

const TAU = Math.PI * 2;
const clamp = (x, lo, hi) => Math.min(hi, Math.max(lo, x));
const smooth = (x) => {
  const t = clamp(x, 0, 1);
  return t * t * (3 - 2 * t);
};

export class Spring {
  constructor(value = 0, frequency = 3.8, damping = 0.82) {
    this.value = value;
    this.velocity = 0;
    this.frequency = frequency;
    this.damping = damping;
  }

  step(target, dt) {
    if (!Number.isFinite(target) || !Number.isFinite(dt) || dt <= 0) return this.value;
    // Semi-implicit Euler at fixed substeps. Overshoot is intentional.
    const omega = TAU * this.frequency;
    const accel = omega * omega * (target - this.value)
      - 2 * this.damping * omega * this.velocity;
    this.velocity += accel * dt;
    this.value += this.velocity * dt;
    return this.value;
  }

  snap(target) {
    this.value = target;
    this.velocity = 0;
  }
}

function randomGenerator(seed) {
  let state = seed >>> 0 || 0x51f1a11;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 4294967296;
  };
}

export class RustyMotion {
  constructor({ seed = 0x16b175, reducedMotion = false } = {}) {
    this.random = randomGenerator(seed);
    this.time = 0;
    this.state = 'idle';
    this.stateTime = 0;
    this.reducedMotion = reducedMotion;
    this.followPointer = true;
    this.pointer = { x: 0, y: 0, until: 0 };
    this.wander = { x: 0, y: 0 };
    this.nextWander = 2.0;
    this.nextBlink = 2.8;
    this.blinkTime = -1;
    this.doubleBlinkQueued = false;
    this.reactionTime = 0;
    this.typing = { user: false, model: false };
    this.cue = null;
    this.motionStrength = 1;
    this.springFrequency = 3.8;
    this.values = Object.fromEntries([
      'headX', 'headY', 'tilt', 'scaleX', 'scaleY',
      'leftX', 'leftY', 'rightX', 'rightY', 'eyeNarrow', 'mouthOpen',
    ].map((name) => [name, new Spring(name.startsWith('scale') ? 1 : 0)]));
  }

  setState(next) {
    if (!STATES.includes(next)) throw new Error(`Unknown motion state: ${next}`);
    if (this.state === next) return;
    this.state = next;
    this.stateTime = 0;
    this.reactionTime = 0;
    if (next === 'happy' || next === 'error') this.reactionTime = 0.001;
  }

  setPointer(x, y) {
    this.pointer.x = clamp(Number.isFinite(x) ? x : 0, -1, 1);
    this.pointer.y = clamp(Number.isFinite(y) ? y : 0, -1, 1);
    this.pointer.until = this.time + 1.9;
  }

  releasePointer() {
    this.pointer.until = this.time + 0.85;
  }

  /** Hook for future UI events. The lab does not monitor external applications. */
  setTyping(source, active) {
    if (source !== 'user' && source !== 'model') throw new Error(`Unknown writer: ${source}`);
    this.typing[source] = Boolean(active);
    if (active) this.cue = null;
  }

  /** A brief, spontaneous-looking distraction; eyes lead the head. */
  noticeSomething(side = 'right') {
    if (side !== 'left' && side !== 'right') throw new Error(`Unknown side: ${side}`);
    this.cue = { kind: 'notice', start: this.time, direction: side === 'right' ? 1 : -1 };
  }

  /** Look back at the person before speaking, then gently settle. */
  prepareResponse() {
    this.typing.model = false;
    this.cue = { kind: 'respond', start: this.time, direction: 0 };
  }

  setReducedMotion(value) {
    this.reducedMotion = Boolean(value);
    if (this.reducedMotion) {
      this.blinkTime = -1;
      this.wander = { x: 0, y: 0 };
    }
  }

  /** Time is supplied by the caller; no wall-clock, external service or timer. */
  update(dt) {
    dt = clamp(Number.isFinite(dt) ? dt : 0, 0, 0.1);
    if (dt === 0) return this.pose();
    const steps = Math.ceil(dt * 120);
    const sub = dt / steps;
    for (let i = 0; i < steps; i++) this.step(sub);
    return this.pose();
  }

  step(dt) {
    this.time += dt;
    this.stateTime += dt;
    const reduced = this.reducedMotion;
    const strength = this.motionStrength;
    const wandering = this.state === 'idle' && !reduced;
    const following = this.followPointer && this.time < this.pointer.until && !reduced;
    const writing = this.typing.user || this.typing.model;

    if (wandering && this.time >= this.nextWander) {
      this.wander.x = (this.random() * 2 - 1) * 0.72;
      this.wander.y = (this.random() * 2 - 1) * 0.36;
      this.nextWander = this.time + 2.4 + this.random() * 3.5;
    }
    if (!reduced && this.time >= this.nextBlink && this.blinkTime < 0) {
      this.blinkTime = 0;
      this.doubleBlinkQueued = this.random() < 0.12;
      this.nextBlink = this.time + 2.9 + this.random() * 3.8;
    }
    if (this.blinkTime >= 0) {
      this.blinkTime += dt;
      if (this.blinkTime >= 0.205) {
        if (this.doubleBlinkQueued) {
          this.blinkTime = -0.11; // short pause before a second blink
          this.doubleBlinkQueued = false;
        } else {
          this.blinkTime = -1;
        }
      }
    } else if (this.blinkTime > -1 && this.blinkTime < 0) {
      this.blinkTime += dt;
      if (this.blinkTime >= 0) this.blinkTime = 0;
    }

    let gazeX = following ? this.pointer.x : wandering ? this.wander.x : 0;
    let gazeY = following ? this.pointer.y : wandering ? this.wander.y : 0;
    let tilt = following ? gazeX * 2.3 : 0;
    let headX = following ? gazeX * 0.8 : 0;
    let headY = 0;
    let scaleX = 1;
    let scaleY = 1;
    let eyeNarrow = 0;
    let mouthOpen = 0;
    switch (this.state) {
      case 'thinking':
        gazeX = 0.65; gazeY = -0.83; tilt = -4.5;
        headY = -1.1; eyeNarrow = 0.06;
        break;
      case 'listening':
        tilt = 4.2; gazeX = following ? gazeX : -0.25;
        mouthOpen = reduced ? 0.38 : 0.25 + 0.25 * Math.sin(this.time * 9);
        break;
      case 'focused':
        gazeX = 0; gazeY = -0.12; tilt = 1.2; eyeNarrow = 0.35;
        break;
      case 'happy': {
        gazeX = 0; gazeY = 0.15; tilt = -2.5; eyeNarrow = 0.28;
        const bounce = reduced ? 0 : Math.exp(-this.stateTime * 2.9)
          * Math.sin(this.stateTime * 18);
        headY = -2.6 * bounce; scaleX = 1 + 0.034 * bounce;
        scaleY = 1 - 0.034 * bounce;
        mouthOpen = 0.22;
        break;
      }
      case 'error': {
        const recoil = reduced ? 0 : Math.exp(-this.stateTime * 5)
          * Math.sin(this.stateTime * 20);
        tilt = -6 + recoil * 3;
        headX = recoil * 2; gazeX = -0.65;
        eyeNarrow = -0.13;
        break;
      }
      case 'sleep':
        gazeX = 0; gazeY = 0; tilt = -7.5; eyeNarrow = 0.92;
        headY = 1;
        break;
      default: break;
    }

    // Attention choreography has precedence over ordinary idle wander and pointer gaze.
    // During writing Rusty studies the text below, even when the pointer is elsewhere.
    if (writing && this.state !== 'sleep') {
      gazeX = this.typing.model ? 0.18 : -0.12;
      gazeY = 0.94;
      headY = 1.55;
      tilt = this.typing.model ? -2.7 : 2.4;
      eyeNarrow = 0.10;
    } else if (this.cue && this.state !== 'sleep') {
      const elapsed = this.time - this.cue.start;
      if (this.cue.kind === 'notice') {
        if (elapsed >= 2.25) this.cue = null;
        else {
          const intensity = smooth(elapsed / 0.16) * (1 - smooth((elapsed - 1.35) / 0.65));
          gazeX = this.cue.direction * 0.91 * intensity;
          gazeY = -0.24 * intensity;
          headX = this.cue.direction * 0.95 * intensity;
          tilt = this.cue.direction * 5.5 * intensity;
        }
      } else if (this.cue.kind === 'respond') {
        if (elapsed >= 1.65) this.cue = null;
        else {
          const intensity = smooth(elapsed / 0.22) * (1 - smooth((elapsed - 1.05) / 0.50));
          gazeX = 0;
          gazeY = -0.08 * intensity;
          headX = 0;
          headY = -0.8 * intensity;
          tilt = -1.6 * intensity;
          eyeNarrow = 0;
        }
      }
    }

    if (!reduced && this.state === 'idle') {
      headY += 0.65 * Math.sin(this.time * 1.45);
      tilt += 0.8 * Math.sin(this.time * 0.74);
      scaleY += 0.004 * Math.sin(this.time * 1.45);
    }
    if (!reduced && this.state === 'sleep') {
      scaleY += 0.009 * Math.sin(this.time * 1.8);
    }
    if (reduced) {
      headX = 0; headY = 0; tilt = 0; scaleX = 1; scaleY = 1;
    }

    const targets = {
      headX: headX * strength, headY: headY * strength, tilt: tilt * strength,
      scaleX: 1 + (scaleX - 1) * strength,
      scaleY: 1 + (scaleY - 1) * strength,
      leftX: gazeX * 3.0 * strength,
      rightX: gazeX * 3.25 * strength,
      leftY: gazeY * 2.5 * strength,
      rightY: gazeY * 2.3 * strength,
      eyeNarrow, mouthOpen,
    };
    for (const [name, spring] of Object.entries(this.values)) {
      const isEye = ['leftX', 'rightX', 'leftY', 'rightY'].includes(name);
      const isHead = ['headX', 'headY', 'tilt'].includes(name);
      spring.frequency = isEye ? this.springFrequency * 1.4
        : isHead ? this.springFrequency * 0.67
        : name === 'mouthOpen' ? 5.4 : this.springFrequency;
      spring.damping = isEye ? 0.91 : 0.79;
      if (reduced) spring.snap(targets[name]);
      else spring.step(targets[name], dt);
    }
  }

  pose() {
    const v = this.values;
    let blink = 0;
    if (!this.reducedMotion && this.blinkTime >= 0) {
      const t = this.blinkTime;
      blink = t < 0.086 ? smooth(t / 0.086) : 1 - smooth((t - 0.086) / 0.119);
    }
    // Emotions and blinks are independent. Both eyes keep their own spring.
    const narrowed = clamp(v.eyeNarrow.value, -0.2, 0.97);
    const eyeOpen = clamp((1 - narrowed) * (1 - blink * 0.96), 0.045, 1.16);
    return {
      state: this.state,
      headX: v.headX.value, headY: v.headY.value,
      tilt: v.tilt.value, scaleX: v.scaleX.value, scaleY: v.scaleY.value,
      leftX: v.leftX.value, leftY: v.leftY.value,
      rightX: v.rightX.value, rightY: v.rightY.value,
      eyeOpen, mouthOpen: clamp(v.mouthOpen.value, 0, 1),
      time: this.time,
    };
  }
}
