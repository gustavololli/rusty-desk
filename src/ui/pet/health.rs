//! Small local status tracker: emit an error once and celebrate only when
//! that same subsystem has actually succeeded again. No network polling.

use super::events::PetEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemArea {
    Ai,
    Tools,
    Storage,
    Voice,
    Music,
    Files,
}

impl SystemArea {
    const fn index(self) -> usize {
        match self {
            Self::Ai => 0,
            Self::Tools => 1,
            Self::Storage => 2,
            Self::Voice => 3,
            Self::Music => 4,
            Self::Files => 5,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PetSystemHealth {
    failed: [bool; 6],
}

impl PetSystemHealth {
    pub fn observe(&mut self, area: SystemArea, success: bool) -> Option<PetEvent> {
        let failed = &mut self.failed[area.index()];
        match (success, *failed) {
            (false, false) => {
                *failed = true;
                Some(PetEvent::ErrorOccurred)
            }
            (true, true) => {
                *failed = false;
                Some(PetEvent::SystemRecovered)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_and_recovery_are_deduplicated_per_subsystem() {
        let mut tracker = PetSystemHealth::default();
        assert_eq!(
            tracker.observe(SystemArea::Ai, false),
            Some(PetEvent::ErrorOccurred)
        );
        assert_eq!(tracker.observe(SystemArea::Ai, false), None);
        assert_eq!(tracker.observe(SystemArea::Storage, true), None);
        assert_eq!(
            tracker.observe(SystemArea::Ai, true),
            Some(PetEvent::SystemRecovered)
        );
        assert_eq!(tracker.observe(SystemArea::Ai, true), None);
        assert_eq!(
            tracker.observe(SystemArea::Voice, false),
            Some(PetEvent::ErrorOccurred)
        );
        assert_eq!(tracker.observe(SystemArea::Music, true), None);
    }
}
