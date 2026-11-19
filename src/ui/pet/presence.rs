//! Short-lived, in-memory awareness of inactivity and long work sessions.
//! Only application UI events and the local Pomodoro timer are observed.

use super::events::PetEvent;

const RETURN_AFTER_SECS: f64 = 5.0 * 60.0;
const TIRED_AFTER_SECS: u32 = 20 * 60;

#[derive(Debug, Default, Clone)]
pub struct PetPresence {
    last_input_at: Option<f64>,
    focus_session: Option<String>,
    tired_notified: bool,
}

impl PetPresence {
    pub fn on_input(&mut self, now: f64) -> Option<PetEvent> {
        if !now.is_finite() {
            return None;
        }
        let returning = self
            .last_input_at
            .is_some_and(|last| now - last >= RETURN_AFTER_SECS);
        self.last_input_at = Some(now);
        returning.then_some(PetEvent::UserReturned)
    }

    pub fn on_focus(&mut self, session_id: Option<&str>, elapsed: u32) -> Option<PetEvent> {
        let Some(session_id) = session_id else {
            self.focus_session = None;
            self.tired_notified = false;
            return None;
        };
        if self.focus_session.as_deref() != Some(session_id) {
            self.focus_session = Some(session_id.to_owned());
            self.tired_notified = false;
        }
        if elapsed >= TIRED_AFTER_SECS && !self.tired_notified {
            self.tired_notified = true;
            return Some(PetEvent::LongWorkSession);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_only_after_real_user_return() {
        let mut p = PetPresence::default();
        assert_eq!(p.on_input(10.0), None); // first touch is not a return
        assert_eq!(p.on_input(290.0), None);
        assert_eq!(p.on_input(591.0), Some(PetEvent::UserReturned));
        assert_eq!(p.on_input(592.0), None);
    }

    #[test]
    fn fatigue_is_once_per_work_session() {
        let mut p = PetPresence::default();
        assert_eq!(p.on_focus(Some("A"), 1199), None);
        assert_eq!(p.on_focus(Some("A"), 1200), Some(PetEvent::LongWorkSession));
        assert_eq!(p.on_focus(Some("A"), 1500), None);
        assert_eq!(p.on_focus(None, 0), None);
        assert_eq!(p.on_focus(Some("B"), 1200), Some(PetEvent::LongWorkSession));
    }
}
