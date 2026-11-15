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
    use super::animation::{AnimationState, ParticleKind};
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
    fn window_mode_variants_are_distinct() {
        assert_ne!(WindowMode::Expanded, WindowMode::Island);
        assert_ne!(WindowMode::Island, WindowMode::Pet);
        assert_ne!(WindowMode::Expanded, WindowMode::Pet);
        assert_eq!(WindowMode::default(), WindowMode::Expanded);
    }
}
