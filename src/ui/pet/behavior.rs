use crate::config::PetConfig;

use super::events::PetEvent;

/// Autonomous curiosity and sleep timer manager for the desktop pet.
#[derive(Debug, Clone)]
pub struct PetBehavior {
    last_interaction_at: f64,
    next_curiosity_at: f64,
    is_sleeping: bool,
}

impl Default for PetBehavior {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl PetBehavior {
    pub fn new(now: f64) -> Self {
        Self {
            last_interaction_at: now,
            next_curiosity_at: now + 30.0,
            is_sleeping: false,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_sleeping(&self) -> bool {
        self.is_sleeping
    }

    /// Records user or system activity, waking the pet if sleeping.
    pub fn record_activity(&mut self, now: f64) {
        self.last_interaction_at = now;
        self.next_curiosity_at = now + 30.0;
        self.is_sleeping = false;
    }

    /// Evaluates autonomous timers and emits at most one low-priority event per tick.
    pub fn tick(&mut self, now: f64, is_busy: bool, config: &PetConfig) -> Option<PetEvent> {
        if is_busy {
            self.last_interaction_at = now;
            self.is_sleeping = false;
            return None;
        }

        if config.sleep_after_mins > 0 && !self.is_sleeping {
            let sleep_secs = f64::from(config.sleep_after_mins) * 60.0;
            if now - self.last_interaction_at >= sleep_secs {
                self.is_sleeping = true;
                return Some(PetEvent::IdleTimeout);
            }
        }

        if !self.is_sleeping
            && config.auto_animations
            && !config.reduce_animations
            && now >= self.next_curiosity_at
        {
            self.next_curiosity_at = now + 30.0;
            return Some(PetEvent::CuriosityTrigger);
        }

        None
    }
}
