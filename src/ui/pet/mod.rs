pub mod animation;
pub mod behavior;
pub mod events;
pub mod renderer;

pub use animation::{AnimationController, AnimationFrame};
pub use behavior::PetBehavior;
pub use events::{PetEvent, WindowMode};
pub use renderer::PetRenderer;

#[cfg(test)]
mod tests {
    use super::animation::{AnimationState, EyeExpression, MouthExpression, ParticleKind};
    use super::*;
    use crate::config::{PetConfig, PetIntensity};
    use crate::state::AssistantMood;

    #[test]
    fn animation_priority_and_glitch_recovery() {
        let mut ctrl = AnimationController::default();
        let cfg = PetConfig::default();

        // Celebrate is High priority; Low CuriosityTrigger must not preempt it.
        ctrl.on_event(PetEvent::TaskCompleted, &cfg, AssistantMood::Idle);
        assert_eq!(ctrl.state(), AnimationState::Celebrate);
        ctrl.on_event(PetEvent::CuriosityTrigger, &cfg, AssistantMood::Idle);
        assert_eq!(ctrl.state(), AnimationState::Celebrate);

        // Critical AiError preempts Celebrate with a brief (<0.5s) Glitch, then Error, then recovers to base state.
        ctrl.on_event(PetEvent::AiError, &cfg, AssistantMood::Focused);
        assert_eq!(ctrl.state(), AnimationState::Glitch);

        for _ in 0..6 {
            ctrl.tick(0.1, AssistantMood::Focused, &cfg);
        }
        assert_eq!(ctrl.state(), AnimationState::Error);

        for _ in 0..12 {
            ctrl.tick(0.1, AssistantMood::Focused, &cfg);
        }
        assert_eq!(ctrl.state(), AnimationState::Focused);
    }

    #[test]
    fn sleep_and_wake_lifecycle() {
        let mut ctrl = AnimationController::default();
        let mut behavior = PetBehavior::new(0.0);
        let cfg = PetConfig {
            sleep_after_mins: 1,
            ..PetConfig::default()
        };

        // Curiosity fires at 30s
        assert_eq!(
            behavior.tick(30.0, false, &cfg),
            Some(PetEvent::CuriosityTrigger)
        );
        // Sleep fires at 60s
        let sleep_ev = behavior.tick(60.0, false, &cfg);
        assert_eq!(sleep_ev, Some(PetEvent::IdleTimeout));
        ctrl.on_event(sleep_ev.unwrap(), &cfg, AssistantMood::Idle);
        assert_eq!(ctrl.state(), AnimationState::Sleep);

        // User click wakes Rusty cleanly
        behavior.record_activity(65.0);
        assert!(!behavior.is_sleeping());
        ctrl.on_event(PetEvent::UserClicked, &cfg, AssistantMood::Idle);
        assert_eq!(ctrl.state(), AnimationState::WakeUp);

        for _ in 0..10 {
            ctrl.tick(0.1, AssistantMood::Idle, &cfg);
        }
        assert_eq!(ctrl.state(), AnimationState::Idle);
    }

    #[test]
    fn reduce_animations_disables_particles_and_bobbing() {
        let mut ctrl = AnimationController::default();
        let cfg = PetConfig {
            reduce_animations: true,
            intensity: PetIntensity::Expressive,
            ..PetConfig::default()
        };

        ctrl.on_event(PetEvent::TaskCompleted, &cfg, AssistantMood::Idle);
        let frame = ctrl.tick(0.05, AssistantMood::Idle, &cfg);
        assert_eq!(frame.bob_px, 0);
        assert_eq!(frame.particle, ParticleKind::None);
        assert_eq!(frame.glitch_scanline, None);
    }

    #[test]
    fn music_mode_enables_headphones_and_groove() {
        let mut ctrl = AnimationController::default();
        let cfg = PetConfig::default();

        ctrl.set_music_playing(true);
        let frame = ctrl.tick(0.1, AssistantMood::Idle, &cfg);
        assert_eq!(ctrl.state(), AnimationState::ListeningMusic);
        assert!(frame.headphones);
        assert_eq!(frame.particle, ParticleKind::MusicNotes);

        ctrl.set_music_playing(false);
        let frame_after = ctrl.tick(0.1, AssistantMood::Idle, &cfg);
        assert_eq!(ctrl.state(), AnimationState::Idle);
        assert!(!frame_after.headphones);
    }

    #[test]
    fn window_mode_variants_are_distinct() {
        assert_ne!(WindowMode::Expanded, WindowMode::Island);
        assert_ne!(WindowMode::Island, WindowMode::Pet);
        assert_ne!(WindowMode::Expanded, WindowMode::Pet);
        assert_eq!(WindowMode::default(), WindowMode::Expanded);
    }

    #[test]
    fn expressions_and_blush_cover_new_eye_and_particle_variants() {
        let mut ctrl = AnimationController::default();
        let cfg = PetConfig::default();

        // Click triggers transient Happy: starts with Wink + blush, then settles to Happy.
        ctrl.on_event(PetEvent::UserClicked, &cfg, AssistantMood::Idle);
        let wink_frame = ctrl.tick(0.1, AssistantMood::Idle, &cfg);
        assert_eq!(wink_frame.eye, EyeExpression::Wink);
        assert_eq!(wink_frame.mouth, MouthExpression::Smile);
        assert!(wink_frame.blush);

        let _ = ctrl.tick(0.25, AssistantMood::Idle, &cfg);
        let happy_frame = ctrl.tick(0.25, AssistantMood::Idle, &cfg);
        assert_eq!(happy_frame.eye, EyeExpression::Happy);
        assert!(happy_frame.blush);

        // Let transient Happy expire back to Idle.
        for _ in 0..8 {
            ctrl.tick(0.1, AssistantMood::Idle, &cfg);
        }
        assert_eq!(ctrl.state(), AnimationState::Idle);

        // LookAround uses Curious asymmetrical eyes.
        ctrl.on_event(PetEvent::UserHovered, &cfg, AssistantMood::Idle);
        let look_frame = ctrl.tick(0.05, AssistantMood::Idle, &cfg);
        assert_eq!(look_frame.eye, EyeExpression::Curious);
        assert!(!look_frame.blush);

        // Thinking keeps Normal elongated eyes looking up-right with ThoughtDots particle.
        ctrl.on_event(PetEvent::AiThinking, &cfg, AssistantMood::Thinking);
        let think_frame = ctrl.tick(0.05, AssistantMood::Thinking, &cfg);
        assert_eq!(think_frame.eye, EyeExpression::Normal);
        assert_eq!(think_frame.look_override_x, Some(1));
        assert_eq!(think_frame.look_override_y, Some(-1));
        assert_eq!(think_frame.particle, ParticleKind::ThoughtDots);

        // Focused uses Squint.
        ctrl.on_event(PetEvent::PomodoroStarted, &cfg, AssistantMood::Focused);
        let focus_frame = ctrl.tick(0.05, AssistantMood::Focused, &cfg);
        assert_eq!(focus_frame.eye, EyeExpression::Squint);

        // Listening uses SoundPulse equalizer bars.
        ctrl.on_event(
            PetEvent::VoiceListeningStart,
            &cfg,
            AssistantMood::Listening,
        );
        let listen_frame = ctrl.tick(0.05, AssistantMood::Listening, &cfg);
        assert_eq!(listen_frame.particle, ParticleKind::SoundPulse);

        // Glitch uses Dizzy eyes when animations are enabled, ErrorCross when reduced.
        ctrl.on_event(PetEvent::AiError, &cfg, AssistantMood::Idle);
        let glitch_frame = ctrl.tick(0.05, AssistantMood::Idle, &cfg);
        assert_eq!(glitch_frame.eye, EyeExpression::Dizzy);
    }

    #[test]
    fn elapsed_advances_and_blink_reaches_the_closed_sprite_frame() {
        use crate::ui::sprites::select_frame;

        let mut ctrl = AnimationController::default();
        let cfg = PetConfig::default();
        let first = ctrl.tick(0.25, AssistantMood::Idle, &cfg);
        assert_eq!(first.state, AnimationState::Idle);
        assert_eq!(first.elapsed_ms, 250);

        let mut saw_blink = false;
        let mut saw_closed = false;
        for _ in 0..80 {
            let frame = ctrl.tick(0.1, AssistantMood::Idle, &cfg);
            if frame.state == AnimationState::Blink {
                saw_blink = true;
                let index =
                    select_frame(frame.elapsed_ms as f32 / 1000.0, 8.0, 4, false, frame.state);
                if index == 1 || index == 2 {
                    saw_closed = true;
                }
            }
        }
        assert!(saw_blink, "idle Rusty should blink");
        assert!(saw_closed, "the blink must reach a closed-eye sprite frame");
    }

    #[test]
    fn reduced_motion_does_not_start_a_blink() {
        let mut ctrl = AnimationController::default();
        let cfg = PetConfig {
            reduce_animations: true,
            ..PetConfig::default()
        };
        for _ in 0..80 {
            let frame = ctrl.tick(0.1, AssistantMood::Idle, &cfg);
            assert_ne!(frame.state, AnimationState::Blink);
            assert_eq!(frame.bob_px, 0);
        }
    }
}
