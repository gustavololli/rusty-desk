use chrono::{Local, NaiveDate};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantMood {
    Idle,
    Listening,
    Thinking,
    Happy,
    Focused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub created_at: String,
}

impl TaskItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            done: false,
            created_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub daily_goal: u32,
    pub current_count: u32,
    pub last_date: NaiveDate,
}

impl HabitItem {
    pub fn new(name: impl Into<String>, icon: impl Into<String>, daily_goal: u32) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            icon: icon.into(),
            daily_goal: daily_goal.max(1),
            current_count: 0,
            last_date: Local::now().date_naive(),
        }
    }

    /// Resets daily count if the date has changed.
    pub fn ensure_today(&mut self) {
        let today = Local::now().date_naive();
        if self.last_date != today {
            self.current_count = 0;
            self.last_date = today;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PomodoroPhase {
    Idle,
    Work,
    Break,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomodoroState {
    pub phase: PomodoroPhase,
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub remaining_secs: u32,
    pub running: bool,
    pub completed_sessions: u32,
}

impl Default for PomodoroState {
    fn default() -> Self {
        Self {
            phase: PomodoroPhase::Idle,
            work_minutes: 25,
            break_minutes: 5,
            remaining_secs: 25 * 60,
            running: false,
            completed_sessions: 0,
        }
    }
}

impl PomodoroState {
    pub fn start_work(&mut self, custom_minutes: Option<u32>) {
        if let Some(m) = custom_minutes {
            self.work_minutes = m.clamp(1, 180);
        }
        self.phase = PomodoroPhase::Work;
        self.remaining_secs = self.work_minutes * 60;
        self.running = true;
    }

    pub fn toggle_pause(&mut self) {
        if self.phase == PomodoroPhase::Idle {
            self.start_work(None);
        } else {
            self.running = !self.running;
        }
    }

    pub fn reset(&mut self) {
        self.phase = PomodoroPhase::Idle;
        self.remaining_secs = self.work_minutes * 60;
        self.running = false;
    }

    /// Advances by 1 second. Returns `Some(new_phase)` if a phase transition occurred.
    pub fn tick_one_second(&mut self) -> Option<PomodoroPhase> {
        if !self.running {
            return None;
        }
        if self.remaining_secs > 1 {
            self.remaining_secs -= 1;
            return None;
        }
        match self.phase {
            PomodoroPhase::Work => {
                self.completed_sessions += 1;
                self.phase = PomodoroPhase::Break;
                self.remaining_secs = self.break_minutes * 60;
                Some(PomodoroPhase::Break)
            }
            PomodoroPhase::Break => {
                self.phase = PomodoroPhase::Idle;
                self.remaining_secs = self.work_minutes * 60;
                self.running = false;
                Some(PomodoroPhase::Idle)
            }
            PomodoroPhase::Idle => None,
        }
    }

    pub fn formatted_time(&self) -> String {
        let mins = self.remaining_secs / 60;
        let secs = self.remaining_secs % 60;
        format!("{mins:02}:{secs:02}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiChatEntry {
    pub is_user: bool,
    pub text: String,
    pub timestamp: String,
}

impl UiChatEntry {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            is_user: true,
            text: text.into(),
            timestamp: Local::now().format("%H:%M").to_string(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            is_user: false,
            text: text.into(),
            timestamp: Local::now().format("%H:%M").to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedChatSession {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub ui_entries: Vec<UiChatEntry>,
}

/// Persistent application data (tasks, habits, pomodoro stats, chat history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    pub tasks: Vec<TaskItem>,
    pub habits: Vec<HabitItem>,
    pub pomodoro: PomodoroState,
    #[serde(default)]
    pub chat_sessions: Vec<SavedChatSession>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            tasks: vec![],
            habits: vec![
                HabitItem::new("Vasos de agua", "💧", 8),
                HabitItem::new("Pausas activas", "🧘", 4),
            ],
            pomodoro: PomodoroState::default(),
            chat_sessions: vec![],
        }
    }
}

impl AppData {
    pub fn upsert_chat_session(&mut self, id: &str, ui_entries: &[UiChatEntry]) {
        let Some(first_user) = ui_entries.iter().find(|e| e.is_user) else {
            return;
        };
        let raw_title = first_user.text.lines().next().unwrap_or("Chat").trim();
        let title: String = if raw_title.chars().count() > 38 {
            let prefix: String = raw_title.chars().take(35).collect();
            format!("{prefix}...")
        } else {
            raw_title.to_string()
        };

        let updated_at = Local::now().format("%d/%m %H:%M").to_string();
        self.chat_sessions.retain(|s| s.id != id);
        self.chat_sessions.insert(
            0,
            SavedChatSession {
                id: id.to_string(),
                title,
                updated_at,
                ui_entries: ui_entries.to_vec(),
            },
        );
        self.chat_sessions.truncate(50);
        let _ = self.save();
    }

    pub fn delete_chat_session(&mut self, id: &str) {
        self.chat_sessions.retain(|s| s.id != id);
        let _ = self.save();
    }
    pub fn data_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
            proj.data_dir().join("data.json")
        } else {
            PathBuf::from("rusty-desk-data.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::data_path();
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(mut data) = serde_json::from_str::<Self>(&raw) {
                for h in &mut data.habits {
                    h.ensure_today();
                }
                data.pomodoro.running = false;
                return data;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::data_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pomodoro_transitions() {
        let mut p = PomodoroState {
            work_minutes: 1,
            break_minutes: 1,
            ..Default::default()
        };
        p.start_work(Some(1));
        assert_eq!(p.phase, PomodoroPhase::Work);
        p.remaining_secs = 1;
        assert_eq!(p.tick_one_second(), Some(PomodoroPhase::Break));
        assert_eq!(p.completed_sessions, 1);
        p.remaining_secs = 1;
        assert_eq!(p.tick_one_second(), Some(PomodoroPhase::Idle));
        assert!(!p.running);
    }
}

