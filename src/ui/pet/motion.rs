//! Lightweight secondary motion for the existing 16-bit sprite renderer.
//! Uses springs on a continuous pose; it never loads or replaces sprite assets.

use crate::config::{PetConfig, PetIntensity};

use super::animation::AnimationState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionPose {
    pub x: f32,
    pub y: f32,
    pub tilt: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub gaze_x: f32,
    pub gaze_y: f32,
}

impl Default for MotionPose {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            tilt: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            gaze_x: 0.0,
            gaze_y: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    const fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    fn step(&mut self, target: f32, dt: f32, hz: f32) {
        let omega = std::f32::consts::TAU * hz;
        self.velocity +=
            (omega * omega * (target - self.value) - 1.65 * omega * self.velocity) * dt;
        self.value += self.velocity * dt;
    }

    fn snap(&mut self, target: f32) {
        self.value = target;
        self.velocity = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
enum Cue {
    Notice { direction: f32, elapsed: f32 },
    Reply { elapsed: f32 },
}

#[derive(Debug, Clone)]
pub struct MotionController {
    time: f32,
    springs: [Spring; 7],
    cue: Option<Cue>,
    writing_user: bool,
    writing_model: bool,
}

impl Default for MotionController {
    fn default() -> Self {
        Self {
            time: 0.0,
            springs: [
                Spring::new(0.0),
                Spring::new(0.0),
                Spring::new(0.0),
                Spring::new(1.0),
                Spring::new(1.0),
                Spring::new(0.0),
                Spring::new(0.0),
            ],
            cue: None,
            writing_user: false,
            writing_model: false,
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl MotionController {
    pub fn notice(&mut self, right: bool) {
        self.cue = Some(Cue::Notice {
            direction: if right { 1.0 } else { -1.0 },
            elapsed: 0.0,
        });
    }

    pub fn prepare_reply(&mut self) {
        self.writing_model = false;
        self.cue = Some(Cue::Reply { elapsed: 0.0 });
    }

    pub fn set_writing(&mut self, user: bool, model: bool) {
        self.writing_user = user;
        self.writing_model = model;
        if user || model {
            self.cue = None;
        }
    }

    pub fn tick(&mut self, dt: f32, state: AnimationState, config: &PetConfig) -> MotionPose {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.time += dt;
        let intensity = match config.intensity {
            PetIntensity::Subtle => 0.55,
            PetIntensity::Normal => 1.0,
            PetIntensity::Expressive => 1.25,
        };
        let mut target = MotionPose::default();
        match state {
            AnimationState::Thinking => {
                target.tilt = -4.5;
                target.y = -1.1;
                target.gaze_x = 0.6;
                target.gaze_y = -0.8;
            }
            AnimationState::Listening => {
                target.tilt = 3.0;
                target.gaze_x = -0.2;
            }
            AnimationState::Focused => {
                target.tilt = 1.0;
                target.gaze_y = -0.1;
            }
            AnimationState::Happy
            | AnimationState::Celebrate
            | AnimationState::PomodoroFinished => {
                target.tilt = -2.2;
            }
            AnimationState::Error | AnimationState::Glitch => {
                target.tilt = -4.0;
                target.gaze_x = -0.5;
            }
            AnimationState::Sleep => {
                target.tilt = -5.0;
                target.y = 1.0;
            }
            AnimationState::LookAround => {
                target.gaze_x = 0.7;
                target.tilt = 2.5;
            }
            _ => {}
        }
        if state == AnimationState::Idle && !config.reduce_animations {
            target.y += 0.45 * (self.time * 1.45).sin();
            target.tilt += 0.55 * (self.time * 0.74).sin();
        }

        // The explicit writing cue takes priority over incidental idle glances.
        if (self.writing_user || self.writing_model) && state != AnimationState::Sleep {
            target.gaze_y = 0.94;
            target.gaze_x = if self.writing_model { 0.18 } else { -0.12 };
            target.y = 1.6;
            target.tilt = if self.writing_model { -2.7 } else { 2.4 };
            target.scale_y = 0.975; // tiny forward nod while reading below
        } else if state != AnimationState::Sleep {
            if let Some(ref mut cue) = self.cue {
                match cue {
                    Cue::Notice { direction, elapsed } => {
                        *elapsed += dt;
                        let amt =
                            smooth(*elapsed / 0.16) * (1.0 - smooth((*elapsed - 1.35) / 0.65));
                        target.gaze_x = *direction * 0.95 * amt;
                        target.x = *direction * 1.0 * amt;
                        target.tilt = *direction * 5.0 * amt;
                        if *elapsed >= 2.25 {
                            self.cue = None;
                        }
                    }
                    Cue::Reply { elapsed } => {
                        *elapsed += dt;
                        let amt = smooth(*elapsed / 0.22) * (1.0 - smooth((*elapsed - 1.05) / 0.5));
                        target.gaze_x = 0.0;
                        target.gaze_y = -0.1 * amt;
                        target.y = -0.8 * amt;
                        target.tilt = -1.6 * amt;
                        if *elapsed >= 1.65 {
                            self.cue = None;
                        }
                    }
                }
            }
        }

        let scaled = [
            target.x * intensity,
            target.y * intensity,
            target.tilt * intensity,
            1.0 + (target.scale_x - 1.0) * intensity,
            1.0 + (target.scale_y - 1.0) * intensity,
            target.gaze_x,
            target.gaze_y,
        ];
        let steps = (dt * 120.0).ceil().max(1.0) as usize;
        for _ in 0..steps {
            for (i, spring) in self.springs.iter_mut().enumerate() {
                if config.reduce_animations {
                    spring.snap(if i < 3 {
                        0.0
                    } else if i < 5 {
                        1.0
                    } else {
                        scaled[i]
                    });
                } else {
                    spring.step(
                        scaled[i],
                        dt / steps as f32,
                        if i >= 5 {
                            5.2
                        } else if i < 3 {
                            2.5
                        } else {
                            3.8
                        },
                    );
                }
            }
        }
        self.pose()
    }

    pub fn pose(&self) -> MotionPose {
        let s = &self.springs;
        MotionPose {
            x: s[0].value,
            y: s[1].value,
            tilt: s[2].value,
            scale_x: s[3].value,
            scale_y: s[4].value,
            gaze_x: s[5].value,
            gaze_y: s[6].value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_looks_down_and_restores() {
        let mut ctrl = MotionController::default();
        let config = PetConfig::default();
        ctrl.set_writing(true, false);
        for _ in 0..120 {
            ctrl.tick(1.0 / 120.0, AnimationState::Idle, &config);
        }
        assert!(ctrl.pose().gaze_y > 0.8);
        assert!(ctrl.pose().y > 1.0);
        ctrl.set_writing(false, false);
        for _ in 0..120 {
            ctrl.tick(1.0 / 120.0, AnimationState::Idle, &config);
        }
        assert!(ctrl.pose().gaze_y.abs() < 0.1);
    }

    #[test]
    fn reduced_motion_disables_head_motion() {
        let mut ctrl = MotionController::default();
        let config = PetConfig {
            reduce_animations: true,
            ..PetConfig::default()
        };
        ctrl.set_writing(true, false);
        ctrl.tick(0.02, AnimationState::Thinking, &config);
        assert_eq!(ctrl.pose().tilt, 0.0);
        assert_eq!(ctrl.pose().y, 0.0);
    }
}
