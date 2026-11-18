//! Experimental, dependency-free Rust motion engine for the yellow Rusty robot.
//!
//! This is intentionally NOT included by `src/main.rs` or any existing UI module.
//! It is a standalone, pure logic draft of `motion.mjs`, not an app integration.
//! No sprites are read or modified. The caller can render the returned `Pose`
//! with a procedural pixel-art renderer at any resolution.

use std::f32::consts::TAU;

const HEAD_X: usize = 0;
const HEAD_Y: usize = 1;
const TILT: usize = 2;
const SCALE_X: usize = 3;
const SCALE_Y: usize = 4;
const LEFT_X: usize = 5;
const LEFT_Y: usize = 6;
const RIGHT_X: usize = 7;
const RIGHT_Y: usize = 8;
const EYE_NARROW: usize = 9;
const MOUTH_OPEN: usize = 10;
const CHANNELS: usize = 11;

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionState {
    #[default]
    Idle,
    Thinking,
    Listening,
    Focused,
    Happy,
    Error,
    Sleep,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub state: MotionState,
    pub head_x: f32,
    pub head_y: f32,
    pub tilt_degrees: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub left_eye_x: f32,
    pub left_eye_y: f32,
    pub right_eye_x: f32,
    pub right_eye_y: f32,
    pub eye_open: f32,
    pub mouth_open: f32,
    pub elapsed_seconds: f32,
}

#[derive(Debug, Clone, Copy)]
struct Spring {
    position: f32,
    velocity: f32,
}

impl Spring {
    const fn new(position: f32) -> Self {
        Self {
            position,
            velocity: 0.0,
        }
    }

    fn advance(&mut self, target: f32, seconds: f32, frequency: f32, damping: f32) {
        let omega = TAU * frequency;
        let acceleration = omega * omega * (target - self.position)
            - 2.0 * damping * omega * self.velocity;
        self.velocity += acceleration * seconds;
        self.position += self.velocity * seconds;
    }

    fn snap(&mut self, target: f32) {
        self.position = target;
        self.velocity = 0.0;
    }
}

/// Owns *only* motion state. No textures, GPU resources, networking, timers,
/// background workers, external services or application configuration.
pub struct MotionEngine {
    state: MotionState,
    elapsed: f32,
    state_elapsed: f32,
    random_state: u32,
    reduced_motion: bool,
    follow_pointer: bool,
    pointer_x: f32,
    pointer_y: f32,
    pointer_until: f32,
    wander_x: f32,
    wander_y: f32,
    next_wander: f32,
    next_blink: f32,
    blink_time: f32,
    double_blink_queued: bool,
    spring_frequency: f32,
    motion_strength: f32,
    channels: [Spring; CHANNELS],
}

impl Default for MotionEngine {
    fn default() -> Self {
        Self::new(0x16b175)
    }
}

impl MotionEngine {
    pub fn new(seed: u32) -> Self {
        let mut channels = [Spring::new(0.0); CHANNELS];
        channels[SCALE_X] = Spring::new(1.0);
        channels[SCALE_Y] = Spring::new(1.0);
        Self {
            state: MotionState::Idle,
            elapsed: 0.0,
            state_elapsed: 0.0,
            random_state: if seed == 0 { 0x051f_1a11 } else { seed },
            reduced_motion: false,
            follow_pointer: true,
            pointer_x: 0.0,
            pointer_y: 0.0,
            pointer_until: 0.0,
            wander_x: 0.0,
            wander_y: 0.0,
            next_wander: 2.0,
            next_blink: 2.8,
            blink_time: -1.0,
            double_blink_queued: false,
            spring_frequency: 3.8,
            motion_strength: 1.0,
            channels,
        }
    }

    pub fn state(&self) -> MotionState {
        self.state
    }

    pub fn set_state(&mut self, next: MotionState) {
        if self.state != next {
            self.state = next;
            self.state_elapsed = 0.0;
        }
    }

    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.pointer_x = if x.is_finite() { x.clamp(-1.0, 1.0) } else { 0.0 };
        self.pointer_y = if y.is_finite() { y.clamp(-1.0, 1.0) } else { 0.0 };
        self.pointer_until = self.elapsed + 1.9;
    }

    pub fn release_pointer(&mut self) {
        self.pointer_until = self.elapsed + 0.85;
    }

    pub fn set_follow_pointer(&mut self, enabled: bool) {
        self.follow_pointer = enabled;
    }

    pub fn set_reduced_motion(&mut self, enabled: bool) {
        self.reduced_motion = enabled;
        if enabled {
            self.blink_time = -1.0;
            self.wander_x = 0.0;
            self.wander_y = 0.0;
        }
    }

    pub fn set_spring_frequency(&mut self, hertz: f32) {
        if hertz.is_finite() {
            self.spring_frequency = hertz.clamp(1.4, 8.0);
        }
    }

    pub fn set_motion_strength(&mut self, strength: f32) {
        if strength.is_finite() {
            self.motion_strength = strength.clamp(0.0, 1.6);
        }
    }

    fn random(&mut self) -> f32 {
        let mut x = self.random_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random_state = x;
        (x as f64 / 4_294_967_296.0) as f32
    }

    /// Advance with a capped delta and 120 Hz substeps. The caller controls
    /// scheduling (e.g. egui repaint) but this experiment does not do that.
    pub fn update(&mut self, seconds: f32) -> Pose {
        if !seconds.is_finite() || seconds <= 0.0 {
            return self.pose();
        }
        let dt = seconds.clamp(0.0, 0.1);
        let steps = (dt * 120.0).ceil().max(1.0) as u32;
        let substep = dt / steps as f32;
        for _ in 0..steps {
            self.step(substep);
        }
        self.pose()
    }

    fn step(&mut self, dt: f32) {
        self.elapsed += dt;
        self.state_elapsed += dt;
        let reduced = self.reduced_motion;
        let wandering = self.state == MotionState::Idle && !reduced;
        let following = self.follow_pointer && self.elapsed < self.pointer_until && !reduced;

        if wandering && self.elapsed >= self.next_wander {
            self.wander_x = (self.random() * 2.0 - 1.0) * 0.72;
            self.wander_y = (self.random() * 2.0 - 1.0) * 0.36;
            self.next_wander = self.elapsed + 2.4 + self.random() * 3.5;
        }
        if !reduced && self.elapsed >= self.next_blink && self.blink_time < 0.0 {
            self.blink_time = 0.0;
            self.double_blink_queued = self.random() < 0.12;
            self.next_blink = self.elapsed + 2.9 + self.random() * 3.8;
        }
        if self.blink_time >= 0.0 {
            self.blink_time += dt;
            if self.blink_time >= 0.205 {
                if self.double_blink_queued {
                    self.blink_time = -0.11;
                    self.double_blink_queued = false;
                } else {
                    self.blink_time = -1.0;
                }
            }
        } else if self.blink_time > -1.0 {
            self.blink_time = (self.blink_time + dt).min(0.0);
        }

        let mut gaze_x = if following {
            self.pointer_x
        } else if wandering {
            self.wander_x
        } else {
            0.0
        };
        let mut gaze_y = if following {
            self.pointer_y
        } else if wandering {
            self.wander_y
        } else {
            0.0
        };
        let mut tilt = if following { gaze_x * 2.3 } else { 0.0 };
        let mut head_x = if following { gaze_x * 0.8 } else { 0.0 };
        let mut head_y = 0.0;
        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        let mut narrow = 0.0;
        let mut mouth = 0.0;

        match self.state {
            MotionState::Thinking => {
                gaze_x = 0.65;
                gaze_y = -0.83;
                tilt = -4.5;
                head_y = -1.1;
                narrow = 0.06;
            }
            MotionState::Listening => {
                tilt = 4.2;
                if !following {
                    gaze_x = -0.25;
                }
                mouth = if reduced {
                    0.38
                } else {
                    0.25 + 0.25 * (self.elapsed * 9.0).sin()
                };
            }
            MotionState::Focused => {
                gaze_x = 0.0;
                gaze_y = -0.12;
                tilt = 1.2;
                narrow = 0.35;
            }
            MotionState::Happy => {
                gaze_x = 0.0;
                gaze_y = 0.15;
                tilt = -2.5;
                narrow = 0.28;
                let bounce = if reduced {
                    0.0
                } else {
                    (-self.state_elapsed * 2.9).exp() * (self.state_elapsed * 18.0).sin()
                };
                head_y = -2.6 * bounce;
                scale_x = 1.0 + 0.034 * bounce;
                scale_y = 1.0 - 0.034 * bounce;
                mouth = 0.22;
            }
            MotionState::Error => {
                let recoil = if reduced {
                    0.0
                } else {
                    (-self.state_elapsed * 5.0).exp() * (self.state_elapsed * 20.0).sin()
                };
                tilt = -6.0 + recoil * 3.0;
                head_x = recoil * 2.0;
                gaze_x = -0.65;
                narrow = -0.13;
            }
            MotionState::Sleep => {
                gaze_x = 0.0;
                gaze_y = 0.0;
                tilt = -7.5;
                narrow = 0.92;
                head_y = 1.0;
            }
            MotionState::Idle => {}
        }

        if !reduced && self.state == MotionState::Idle {
            head_y += 0.65 * (self.elapsed * 1.45).sin();
            tilt += 0.8 * (self.elapsed * 0.74).sin();
            scale_y += 0.004 * (self.elapsed * 1.45).sin();
        }
        if !reduced && self.state == MotionState::Sleep {
            scale_y += 0.009 * (self.elapsed * 1.8).sin();
        }
        if reduced {
            head_x = 0.0;
            head_y = 0.0;
            tilt = 0.0;
            scale_x = 1.0;
            scale_y = 1.0;
        }

        let s = self.motion_strength;
        let targets = [
            head_x * s,
            head_y * s,
            tilt * s,
            1.0 + (scale_x - 1.0) * s,
            1.0 + (scale_y - 1.0) * s,
            gaze_x * 3.0 * s,
            gaze_y * 2.5 * s,
            gaze_x * 3.25 * s,
            gaze_y * 2.3 * s,
            narrow,
            mouth,
        ];
        for (index, channel) in self.channels.iter_mut().enumerate() {
            let hz = match index {
                LEFT_X | RIGHT_X => self.spring_frequency * 1.1,
                MOUTH_OPEN => 5.4,
                _ => self.spring_frequency,
            };
            let damping = if index == LEFT_X || index == RIGHT_X {
                0.93
            } else {
                0.79
            };
            if reduced {
                channel.snap(targets[index]);
            } else {
                channel.advance(targets[index], dt, hz, damping);
            }
        }
    }

    pub fn pose(&self) -> Pose {
        let blink = if !self.reduced_motion && self.blink_time >= 0.0 {
            let t = self.blink_time;
            if t < 0.086 {
                smooth(t / 0.086)
            } else {
                1.0 - smooth((t - 0.086) / 0.119)
            }
        } else {
            0.0
        };
        let narrow = self.channels[EYE_NARROW].position.clamp(-0.2, 0.97);
        Pose {
            state: self.state,
            head_x: self.channels[HEAD_X].position,
            head_y: self.channels[HEAD_Y].position,
            tilt_degrees: self.channels[TILT].position,
            scale_x: self.channels[SCALE_X].position,
            scale_y: self.channels[SCALE_Y].position,
            left_eye_x: self.channels[LEFT_X].position,
            left_eye_y: self.channels[LEFT_Y].position,
            right_eye_x: self.channels[RIGHT_X].position,
            right_eye_y: self.channels[RIGHT_Y].position,
            eye_open: ((1.0 - narrow) * (1.0 - blink * 0.96)).clamp(0.045, 1.16),
            mouth_open: self.channels[MOUTH_OPEN].position.clamp(0.0, 1.0),
            elapsed_seconds: self.elapsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advance(engine: &mut MotionEngine, seconds: f32) -> Pose {
        let steps = (seconds * 120.0).round() as u32;
        for _ in 0..steps {
            engine.update(1.0 / 120.0);
        }
        engine.pose()
    }

    #[test]
    fn eyes_follow_pointer_independently() {
        let mut engine = MotionEngine::default();
        engine.set_pointer(1.0, -1.0);
        let pose = advance(&mut engine, 0.5);
        assert!(pose.left_eye_x > 2.0);
        assert!(pose.right_eye_x > pose.left_eye_x);
        assert!(pose.left_eye_y < pose.right_eye_y);
    }

    #[test]
    fn thinking_settles_without_teleporting() {
        let mut engine = MotionEngine::default();
        engine.set_state(MotionState::Thinking);
        let first = engine.update(1.0 / 120.0);
        let later = advance(&mut engine, 1.2);
        assert!(first.tilt_degrees.abs() < later.tilt_degrees.abs());
        assert!(later.left_eye_y < -1.0);
    }

    #[test]
    fn reduced_motion_preserves_static_expressions() {
        let mut engine = MotionEngine::default();
        engine.set_reduced_motion(true);
        engine.set_pointer(1.0, -1.0);
        engine.set_state(MotionState::Sleep);
        let pose = advance(&mut engine, 1.0);
        assert_eq!(pose.tilt_degrees, 0.0);
        assert_eq!(pose.head_y, 0.0);
        assert!(pose.eye_open < 0.2);
    }

    #[test]
    fn seeded_idle_behaviour_is_reproducible() {
        let mut left = MotionEngine::new(9876);
        let mut right = MotionEngine::new(9876);
        for _ in 0..1200 {
            assert_eq!(left.update(1.0 / 120.0), right.update(1.0 / 120.0));
        }
    }

    #[test]
    fn bad_delta_never_changes_time() {
        let mut engine = MotionEngine::default();
        assert_eq!(engine.update(f32::NAN).elapsed_seconds, 0.0);
        assert_eq!(engine.update(-1.0).elapsed_seconds, 0.0);
        assert!(engine.update(500.0).elapsed_seconds <= 0.1001);
    }
}
