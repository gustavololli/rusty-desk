use crate::config::{PetConfig, PetIntensity};
use crate::state::AssistantMood;

use super::events::{EventPriority, PetEvent};

/// High-level procedural animation states for Rusty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationState {
    #[default]
    Idle,
    Blink,
    LookAround,
    Sleep,
    WakeUp,
    Thinking,
    Listening,
    Happy,
    Celebrate,
    Focused,
    Error,
    Glitch,
    PomodoroFinished,
    ListeningMusic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EyeExpression {
    Normal,
    Closed,
    Happy,
    Wink,
    Curious,
    Squint,
    WideAlert,
    ErrorCross,
    Dizzy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouthExpression {
    Neutral,
    Smile,
    TalkOpen,
    SleepSmall,
    Surprised,
    Frown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleKind {
    None,
    Sparkles,
    SleepZzz,
    Exclamation,
    Question,
    SoundPulse,
    MusicNotes,
    ThoughtDots,
}

/// Output of one quantized animation tick consumed by the pixel renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationFrame {
    pub state: AnimationState,
    pub bob_px: i32,
    pub offset_x_px: i32,
    pub offset_y_px: i32,
    pub look_override_x: Option<i32>,
    pub look_override_y: Option<i32>,
    pub glitch_scanline: Option<i32>,
    pub eye: EyeExpression,
    pub mouth: MouthExpression,
    pub blush: bool,
    pub particle: ParticleKind,
    pub particle_step: u8,
    pub accent_override: Option<[u8; 3]>,
    pub headphones: bool,
}

/// Procedural 8-bit animation state machine. Runs 100% locally with zero allocations.
#[derive(Debug, Clone)]
pub struct AnimationController {
    state: AnimationState,
    priority: EventPriority,
    state_elapsed: f32,
    state_duration: Option<f32>,
    total_time: f32,
    frame_index: u32,
    blink_timer: f32,
    next_blink_in: f32,
    look_dir_x: i32,
    rng_state: u32,
    is_playing_music: bool,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self {
            state: AnimationState::Idle,
            priority: EventPriority::Low,
            state_elapsed: 0.0,
            state_duration: None,
            total_time: 0.0,
            frame_index: 0,
            blink_timer: 0.0,
            next_blink_in: 3.4,
            look_dir_x: 1,
            rng_state: 0x5A17_93E1,
            is_playing_music: false,
        }
    }
}

impl AnimationController {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn state(&self) -> AnimationState {
        self.state
    }

    pub fn set_music_playing(&mut self, playing: bool) {
        if self.is_playing_music == playing {
            return;
        }
        self.is_playing_music = playing;
        if playing && self.state == AnimationState::Sleep {
            self.transition(AnimationState::ListeningMusic, EventPriority::Low, None);
        }
    }

    fn next_rand_unit(&mut self) -> f32 {
        // Fast deterministic LCG for variable blink/curiosity timing.
        self.rng_state = self
            .rng_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.rng_state >> 16) as f32) / 65535.0
    }

    fn base_state_for_mood(mood: AssistantMood) -> AnimationState {
        match mood {
            AssistantMood::Thinking => AnimationState::Thinking,
            AssistantMood::Listening => AnimationState::Listening,
            AssistantMood::Focused => AnimationState::Focused,
            AssistantMood::Happy => AnimationState::Happy,
            AssistantMood::Idle => AnimationState::Idle,
        }
    }

    fn transition(&mut self, next: AnimationState, priority: EventPriority, duration: Option<f32>) {
        self.state = next;
        self.priority = priority;
        self.state_elapsed = 0.0;
        self.state_duration = duration;
    }

    /// Processes a semantic event, respecting priority rules and user preferences.
    pub fn on_event(&mut self, event: PetEvent, config: &PetConfig, mood: AssistantMood) {
        let ev_prio = event.priority();

        // Transient critical/high states can only be preempted by equal or higher priority,
        // except explicit state-resetting lifecycle events.
        let is_transient = self.state_duration.is_some();
        if is_transient && ev_prio < self.priority && !matches!(event, PetEvent::UserClicked) {
            return;
        }

        let intensity_scale = match config.intensity {
            PetIntensity::Subtle => 0.7,
            PetIntensity::Normal => 1.0,
            PetIntensity::Expressive => 1.35,
        };

        match event {
            PetEvent::UserClicked => {
                if self.state == AnimationState::Sleep {
                    self.transition(AnimationState::WakeUp, EventPriority::High, Some(0.75));
                } else {
                    self.transition(AnimationState::Happy, EventPriority::High, Some(1.0));
                }
            }
            PetEvent::UserHovered => {
                if self.state == AnimationState::Idle {
                    self.look_dir_x = if self.next_rand_unit() > 0.5 { 1 } else { -1 };
                    self.transition(AnimationState::LookAround, EventPriority::Low, Some(0.9));
                }
            }
            PetEvent::UserDragged => {
                if self.state == AnimationState::Sleep {
                    self.transition(AnimationState::WakeUp, EventPriority::Normal, Some(0.6));
                }
            }
            PetEvent::AiThinking => {
                self.transition(AnimationState::Thinking, EventPriority::High, None);
            }
            PetEvent::AiReplyReady => {
                self.transition(
                    AnimationState::Happy,
                    EventPriority::High,
                    Some(1.6 * intensity_scale),
                );
            }
            PetEvent::AiError | PetEvent::ErrorOccurred => {
                if config.reduce_animations {
                    self.transition(AnimationState::Error, EventPriority::Critical, Some(1.2));
                } else {
                    // Quick non-blocking glitch (< 0.45s) before clean recovery.
                    self.transition(AnimationState::Glitch, EventPriority::Critical, Some(0.42));
                }
            }
            PetEvent::TaskCompleted | PetEvent::HabitCompleted => {
                if config.celebrate_tasks {
                    self.transition(
                        AnimationState::Celebrate,
                        EventPriority::High,
                        Some(2.0 * intensity_scale),
                    );
                } else {
                    self.transition(AnimationState::Happy, EventPriority::High, Some(1.2));
                }
            }
            PetEvent::MarkdownConverted => {
                self.transition(
                    AnimationState::Happy,
                    EventPriority::High,
                    Some(1.5 * intensity_scale),
                );
            }
            PetEvent::PomodoroStarted => {
                self.transition(AnimationState::Focused, EventPriority::Normal, None);
            }
            PetEvent::PomodoroPaused => {
                let target = Self::base_state_for_mood(mood);
                self.transition(target, EventPriority::Low, None);
            }
            PetEvent::PomodoroFinished => {
                self.transition(
                    AnimationState::PomodoroFinished,
                    EventPriority::Critical,
                    Some(2.8),
                );
            }
            PetEvent::VoiceListeningStart => {
                self.transition(AnimationState::Listening, EventPriority::High, None);
            }
            PetEvent::VoiceListeningStop => {
                let target = Self::base_state_for_mood(mood);
                self.transition(target, EventPriority::Low, None);
            }
            PetEvent::IdleTimeout => {
                if config.sleep_after_mins > 0
                    && matches!(self.state, AnimationState::Idle | AnimationState::Blink)
                {
                    self.transition(AnimationState::Sleep, EventPriority::Low, None);
                }
            }
            PetEvent::CuriosityTrigger => {
                if config.auto_animations
                    && !config.reduce_animations
                    && self.state == AnimationState::Idle
                {
                    self.look_dir_x = if self.next_rand_unit() > 0.5 { 2 } else { -2 };
                    self.transition(AnimationState::LookAround, EventPriority::Low, Some(1.3));
                }
            }
            PetEvent::ModeChanged(_) => {
                if self.state == AnimationState::Sleep {
                    self.transition(AnimationState::WakeUp, EventPriority::Normal, Some(0.65));
                }
            }
        }
    }

    /// Advances the procedural state machine by `dt` seconds and produces a quantized pixel frame.
    pub fn tick(&mut self, dt: f32, mood: AssistantMood, config: &PetConfig) -> AnimationFrame {
        let dt = dt.clamp(0.0, 0.25);
        self.total_time += dt;
        self.state_elapsed += dt;
        self.frame_index = (self.total_time * 12.0) as u32;

        // Sync continuous mood transitions cleanly when not in transient/sleep states.
        if self.state_duration.is_none() && self.state != AnimationState::Sleep {
            let expected = if self.is_playing_music && mood == AssistantMood::Idle {
                AnimationState::ListeningMusic
            } else if self.is_playing_music && mood == AssistantMood::Happy {
                AnimationState::ListeningMusic
            } else {
                Self::base_state_for_mood(mood)
            };
            if expected != self.state {
                self.state = expected;
                self.priority = EventPriority::Low;
            }
        }

        // Handle transient state expiration and recovery.
        if let Some(dur) = self.state_duration {
            if self.state_elapsed >= dur {
                if self.state == AnimationState::Glitch {
                    // Brief glitch settles into short Error indicator, then recovers.
                    self.transition(AnimationState::Error, EventPriority::Normal, Some(0.85));
                } else {
                    let recovery = Self::base_state_for_mood(mood);
                    self.transition(recovery, EventPriority::Low, None);
                }
            }
        }

        // Variable-interval natural blinking in calm states.
        if !config.reduce_animations
            && matches!(self.state, AnimationState::Idle | AnimationState::Focused)
        {
            self.blink_timer += dt;
            if self.blink_timer >= self.next_blink_in {
                self.blink_timer = 0.0;
                self.next_blink_in = 2.6 + self.next_rand_unit() * 3.8;
                if self.state == AnimationState::Idle {
                    self.transition(AnimationState::Blink, EventPriority::Low, Some(0.16));
                }
            }
        }

        self.build_frame(config)
    }

    fn build_frame(&self, config: &PetConfig) -> AnimationFrame {
        let reduced = config.reduce_animations;
        let expressive = config.intensity == PetIntensity::Expressive;
        let step = (self.frame_index % 8) as u8;

        let default_bob = if reduced {
            0
        } else {
            ((self.total_time * 3.0).sin() * 0.6).round() as i32
        };

        let mut frame = AnimationFrame {
            state: self.state,
            bob_px: default_bob,
            offset_x_px: 0,
            offset_y_px: 0,
            look_override_x: None,
            look_override_y: None,
            glitch_scanline: None,
            eye: EyeExpression::Normal,
            mouth: MouthExpression::Neutral,
            blush: false,
            particle: ParticleKind::None,
            particle_step: step,
            accent_override: None,
            headphones: self.is_playing_music,
        };

        match self.state {
            AnimationState::Idle => {}
            AnimationState::Blink => {
                frame.eye = EyeExpression::Closed;
            }
            AnimationState::LookAround => {
                frame.eye = EyeExpression::Curious;
                frame.look_override_x = Some(self.look_dir_x);
                frame.look_override_y = Some(0);
            }
            AnimationState::Sleep => {
                frame.bob_px = 0;
                frame.offset_y_px = if reduced { 0 } else { 1 };
                frame.eye = EyeExpression::Closed;
                frame.mouth = MouthExpression::SleepSmall;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::SleepZzz
                };
                frame.accent_override = Some([96, 165, 250]);
            }
            AnimationState::WakeUp => {
                frame.bob_px = if reduced { 0 } else { -1 };
                frame.eye = EyeExpression::WideAlert;
                frame.mouth = MouthExpression::Surprised;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::Exclamation
                };
            }
            AnimationState::Thinking => {
                frame.eye = EyeExpression::Normal;
                frame.look_override_x = Some(1);
                frame.look_override_y = Some(-1);
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::ThoughtDots
                };
            }
            AnimationState::Listening => {
                frame.mouth = if (self.frame_index / 2).is_multiple_of(2) {
                    MouthExpression::TalkOpen
                } else {
                    MouthExpression::Neutral
                };
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::SoundPulse
                };
            }
            AnimationState::Happy => {
                frame.eye =
                    if !reduced && self.state_duration.is_some() && self.state_elapsed < 0.45 {
                        EyeExpression::Wink
                    } else {
                        EyeExpression::Happy
                    };
                frame.mouth = MouthExpression::Smile;
                frame.blush = true;
                if !reduced && expressive {
                    frame.offset_y_px = if (self.frame_index / 2).is_multiple_of(2) {
                        -1
                    } else {
                        0
                    };
                }
            }
            AnimationState::Celebrate => {
                frame.eye = EyeExpression::Happy;
                frame.mouth = MouthExpression::Smile;
                frame.blush = true;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::Sparkles
                };
                if !reduced {
                    frame.offset_y_px = match self.frame_index % 4 {
                        0 | 1 => -1,
                        _ => 0,
                    };
                }
                frame.accent_override = Some([52, 211, 153]);
            }
            AnimationState::Focused => {
                frame.eye = EyeExpression::Squint;
            }
            AnimationState::Error => {
                frame.eye = EyeExpression::ErrorCross;
                frame.mouth = MouthExpression::Frown;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::Question
                };
                frame.accent_override = Some([248, 113, 113]);
            }
            AnimationState::Glitch => {
                frame.eye = if reduced {
                    EyeExpression::ErrorCross
                } else {
                    EyeExpression::Dizzy
                };
                frame.mouth = MouthExpression::Frown;
                if !reduced {
                    frame.offset_x_px = if self.frame_index.is_multiple_of(2) {
                        1
                    } else {
                        -1
                    };
                    frame.glitch_scanline = Some(((self.frame_index * 5) % 16 + 2) as i32);
                }
                frame.accent_override = Some([244, 63, 94]);
            }
            AnimationState::PomodoroFinished => {
                frame.eye = EyeExpression::Happy;
                frame.mouth = MouthExpression::Smile;
                frame.blush = true;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::Exclamation
                };
                if !reduced {
                    frame.offset_y_px = if (self.frame_index / 2).is_multiple_of(2) {
                        -1
                    } else {
                        0
                    };
                }
                frame.accent_override = Some([52, 211, 153]);
            }
            AnimationState::ListeningMusic => {
                frame.eye = EyeExpression::Happy;
                frame.mouth = MouthExpression::Smile;
                frame.blush = true;
                frame.particle = if reduced {
                    ParticleKind::None
                } else {
                    ParticleKind::MusicNotes
                };
                if !reduced {
                    // Rhythmic head-bob + side-to-side groove on the beat
                    frame.bob_px = match self.frame_index % 4 {
                        1 | 2 => -1,
                        _ => 0,
                    };
                    frame.offset_x_px = match (self.frame_index / 4) % 4 {
                        0 => -1,
                        2 => 1,
                        _ => 0,
                    };
                }
            }
        }

        frame
    }

    /// Adaptive repaint interval to minimize CPU/GPU usage when calm or sleeping.
    pub fn recommended_repaint_ms(&self, is_hovered: bool, config: &PetConfig) -> u64 {
        if config.reduce_animations {
            return if self.state_duration.is_some() {
                66
            } else {
                250
            };
        }
        if is_hovered
            || self.state_duration.is_some()
            || matches!(
                self.state,
                AnimationState::Thinking
                    | AnimationState::Listening
                    | AnimationState::Celebrate
                    | AnimationState::PomodoroFinished
                    | AnimationState::Glitch
                    | AnimationState::ListeningMusic
            )
        {
            33
        } else if self.state == AnimationState::Sleep {
            220
        } else {
            90
        }
    }
}
