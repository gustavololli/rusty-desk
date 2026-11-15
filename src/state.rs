use crate::ai::{ChatMessage, ToolCall};
use crate::persistence::{
    ConversationSummary, Database, MessageKind, MigrationOutcome, NewPersistedMessage,
    NewToolExecution, PersistedMessage,
};
use chrono::{DateTime, Local, NaiveDate, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
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
            created_at: Utc::now().to_rfc3339(),
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

/// Persistent application data backed by local SQLite (`AppData`).
pub struct AppData {
    pub tasks: Vec<TaskItem>,
    pub habits: Vec<HabitItem>,
    pub pomodoro: PomodoroState,
    pub db: Option<Database>,
    pub startup_error: Option<String>,
    pub migration_notice: Option<String>,
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
            db: None,
            startup_error: None,
            migration_notice: None,
        }
    }
}

fn format_local_time_from_utc(utc_str: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(utc_str) {
        return dt.with_timezone(&Local).format("%H:%M").to_string();
    }
    Local::now().format("%H:%M").to_string()
}

pub fn format_local_datetime_from_utc(utc_str: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(utc_str) {
        return dt.with_timezone(&Local).format("%d/%m %H:%M").to_string();
    }
    utc_str.to_string()
}

fn phase_to_str(phase: PomodoroPhase) -> &'static str {
    match phase {
        PomodoroPhase::Idle => "Idle",
        PomodoroPhase::Work => "Work",
        PomodoroPhase::Break => "Break",
    }
}

fn str_to_phase(s: &str) -> PomodoroPhase {
    match s {
        "Work" => PomodoroPhase::Work,
        "Break" => PomodoroPhase::Break,
        _ => PomodoroPhase::Idle,
    }
}

impl AppData {
    pub fn legacy_json_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
            proj.data_dir().join("data.json")
        } else {
            PathBuf::from("rusty-desk-data.json")
        }
    }

    pub fn load() -> Self {
        let db_path = Database::default_db_path();
        let json_path = Self::legacy_json_path();
        match Database::open_with_json_migration(&db_path, &json_path) {
            Ok((db, outcome)) => match Self::from_database(db) {
                Ok(mut data) => {
                    if let MigrationOutcome::Migrated {
                        tasks,
                        habits,
                        conversations,
                        ..
                    } = outcome
                    {
                        data.migration_notice = Some(format!(
                            "Migración SQLite completada: {tasks} tarea(s), {habits} hábito(s), {conversations} chat(s)."
                        ));
                    }
                    data
                }
                Err(e) => {
                    let mut fallback = Self::default();
                    fallback.startup_error = Some(format!("Error cargando SQLite: {e}"));
                    fallback
                }
            },
            Err(e) => {
                let mut fallback = Self::default();
                fallback.startup_error = Some(format!("Error abriendo/migrando SQLite: {e}"));
                fallback
            }
        }
    }

    pub fn from_database(db: Database) -> anyhow::Result<Self> {
        let persisted_tasks = db.list_tasks()?;
        let tasks: Vec<TaskItem> = persisted_tasks
            .into_iter()
            .map(|t| TaskItem {
                id: t.id,
                title: t.title,
                done: t.done,
                created_at: t.created_at,
            })
            .collect();

        let persisted_habits = db.list_habits()?;
        let mut habits: Vec<HabitItem> = persisted_habits
            .into_iter()
            .map(|h| {
                let mut item = HabitItem {
                    id: h.id,
                    name: h.name,
                    icon: h.icon,
                    daily_goal: h.daily_goal,
                    current_count: h.current_count,
                    last_date: h.last_date,
                };
                item.ensure_today();
                item
            })
            .collect();

        if habits.is_empty() {
            let defaults = vec![
                HabitItem::new("Vasos de agua", "💧", 8),
                HabitItem::new("Pausas activas", "🧘", 4),
            ];
            for h in &defaults {
                db.upsert_habit(&h.id, &h.name, &h.icon, h.daily_goal, h.current_count, h.last_date)?;
            }
            habits = defaults;
        }

        let pomodoro = match db.load_pomodoro()? {
            Some(p) => PomodoroState {
                phase: str_to_phase(&p.phase),
                work_minutes: p.work_minutes,
                break_minutes: p.break_minutes,
                remaining_secs: p.remaining_secs,
                running: false,
                completed_sessions: p.completed_sessions,
            },
            None => {
                let def = PomodoroState::default();
                db.save_pomodoro(
                    phase_to_str(def.phase),
                    def.work_minutes,
                    def.break_minutes,
                    def.remaining_secs,
                    def.completed_sessions,
                )?;
                def
            }
        };

        Ok(Self {
            tasks,
            habits,
            pomodoro,
            db: Some(db),
            startup_error: None,
            migration_notice: None,
        })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        for t in &self.tasks {
            db.upsert_task(&t.id, &t.title, t.done, &t.created_at)?;
        }
        for h in &self.habits {
            db.upsert_habit(&h.id, &h.name, &h.icon, h.daily_goal, h.current_count, h.last_date)?;
        }
        db.save_pomodoro(
            phase_to_str(self.pomodoro.phase),
            self.pomodoro.work_minutes,
            self.pomodoro.break_minutes,
            self.pomodoro.remaining_secs,
            self.pomodoro.completed_sessions,
        )?;
        Ok(())
    }

    pub fn upsert_task(&self, task: &TaskItem) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        db.upsert_task(&task.id, &task.title, task.done, &task.created_at)
    }

    pub fn delete_task_at(&mut self, idx: usize) -> anyhow::Result<()> {
        if idx >= self.tasks.len() {
            return Ok(());
        }
        let id = self.tasks[idx].id.clone();
        if let Some(db) = &self.db {
            db.soft_delete_task(&id)?;
        }
        self.tasks.remove(idx);
        Ok(())
    }

    pub fn upsert_habit(&self, habit: &HabitItem) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        db.upsert_habit(
            &habit.id,
            &habit.name,
            &habit.icon,
            habit.daily_goal,
            habit.current_count,
            habit.last_date,
        )
    }

    pub fn log_habit_delta(&mut self, idx: usize, delta: i32) -> anyhow::Result<()> {
        let Some(h) = self.habits.get_mut(idx) else {
            anyhow::bail!("Hábito no encontrado");
        };
        h.ensure_today();
        let next = if delta >= 0 {
            h.current_count.saturating_add(delta as u32)
        } else {
            h.current_count.saturating_sub((-delta) as u32)
        };
        h.current_count = next;
        let id = h.id.clone();
        let last_date = h.last_date;
        if let Some(db) = &mut self.db {
            db.log_habit_entry(&id, last_date, delta as i64, next)?;
        }
        Ok(())
    }

    pub fn save_pomodoro(&self) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        db.save_pomodoro(
            phase_to_str(self.pomodoro.phase),
            self.pomodoro.work_minutes,
            self.pomodoro.break_minutes,
            self.pomodoro.remaining_secs,
            self.pomodoro.completed_sessions,
        )
    }

    pub fn list_conversations_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<ConversationSummary>> {
        let Some(db) = &self.db else {
            return Ok(Vec::new());
        };
        db.list_conversations_page(limit, offset)
    }

    pub fn count_conversations(&self) -> anyhow::Result<usize> {
        let Some(db) = &self.db else {
            return Ok(0);
        };
        db.count_conversations()
    }

    pub fn delete_conversation(&mut self, id: &str) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        db.soft_delete_conversation(id)
    }

    pub fn record_chat_message(
        &mut self,
        conversation_id: &str,
        title_hint: Option<&str>,
        msg: &ChatMessage,
        kind: MessageKind,
    ) -> anyhow::Result<String> {
        let Some(db) = &mut self.db else {
            return Ok(Uuid::new_v4().to_string());
        };

        let title = title_hint
            .map(|raw| {
                let first_line = raw.lines().next().unwrap_or("Chat").trim();
                if first_line.chars().count() > 38 {
                    let prefix: String = first_line.chars().take(35).collect();
                    format!("{prefix}...")
                } else if first_line.is_empty() {
                    "Nuevo chat".to_string()
                } else {
                    first_line.to_string()
                }
            })
            .unwrap_or_else(|| "Nuevo chat".to_string());

        db.ensure_conversation(conversation_id, &title)?;
        if let Some(hint) = title_hint {
            if !hint.trim().is_empty() {
                // Keep title updated if conversation was created with placeholder title
                let existing = db.list_messages(conversation_id)?;
                if existing.iter().all(|m| m.role != "user") && msg.role == "user" {
                    db.update_conversation_title(conversation_id, &title)?;
                }
            }
        }

        let msg_id = Uuid::new_v4().to_string();
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let raw_gemini_parts_json = msg
            .raw_gemini_parts
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        db.append_message(&NewPersistedMessage {
            id: msg_id.clone(),
            conversation_id: conversation_id.to_string(),
            role: msg.role.clone(),
            kind,
            content: msg.content.clone(),
            tool_calls_json,
            tool_call_id: msg.tool_call_id.clone(),
            raw_gemini_parts_json,
            created_at: Utc::now().to_rfc3339(),
        })?;
        Ok(msg_id)
    }

    pub fn has_tool_execution(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
    ) -> anyhow::Result<bool> {
        let Some(db) = &self.db else {
            return Ok(false);
        };
        db.has_tool_execution(conversation_id, tool_call_id)
    }

    pub fn record_tool_execution(&self, exec: &NewToolExecution) -> anyhow::Result<()> {
        let Some(db) = &self.db else {
            return Ok(());
        };
        db.record_tool_execution(exec)
    }

    /// Derives both visual chat entries and context-optimized AI `ChatMessage` history
    /// from the single persisted `messages` table.
    pub fn load_conversation_history(
        &self,
        conversation_id: &str,
        system_prompt: &str,
        max_turns: usize,
    ) -> anyhow::Result<(Vec<UiChatEntry>, Vec<ChatMessage>)> {
        let Some(db) = &self.db else {
            return Ok((Vec::new(), vec![ChatMessage::system(system_prompt)]));
        };
        let persisted = db.list_messages(conversation_id)?;
        Ok(build_dual_history(&persisted, system_prompt, max_turns))
    }
}

/// Derives visual history and context-optimized technical history from persisted messages,
/// keeping tool_calls and their corresponding tool results grouped atomically.
pub fn build_dual_history(
    persisted: &[PersistedMessage],
    system_prompt: &str,
    max_turns: usize,
) -> (Vec<UiChatEntry>, Vec<ChatMessage>) {
    let mut ui_entries = Vec::new();
    let mut tech_turns: Vec<Vec<ChatMessage>> = Vec::new();

    for m in persisted {
        let local_ts = format_local_time_from_utc(&m.created_at);
        match m.role.as_str() {
            "user" => {
                if let Some(text) = &m.content {
                    ui_entries.push(UiChatEntry {
                        is_user: true,
                        text: text.clone(),
                        timestamp: local_ts,
                    });
                    tech_turns.push(vec![ChatMessage::user(text)]);
                }
            }
            "assistant" => {
                let tool_calls: Option<Vec<ToolCall>> = m
                    .tool_calls_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                let raw_gemini_parts: Option<Vec<serde_json::Value>> = m
                    .raw_gemini_parts_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());

                if let Some(text) = &m.content {
                    if !text.trim().is_empty() {
                        ui_entries.push(UiChatEntry {
                            is_user: false,
                            text: text.clone(),
                            timestamp: local_ts,
                        });
                    }
                }

                let chat_msg = ChatMessage {
                    role: "assistant".to_string(),
                    content: m.content.clone(),
                    tool_calls,
                    tool_call_id: None,
                    raw_gemini_parts,
                };
                tech_turns.push(vec![chat_msg]);
            }
            "tool" => {
                let content = m.content.clone().unwrap_or_default();
                let call_id = m.tool_call_id.clone().unwrap_or_default();
                if m.kind == MessageKind::ToolError {
                    ui_entries.push(UiChatEntry {
                        is_user: false,
                        text: format!("⚠️ {content}"),
                        timestamp: local_ts,
                    });
                }
                let tool_msg = ChatMessage::tool_result(call_id, content);
                // Group tool results with the preceding assistant tool_call turn so they are never split!
                if let Some(last_turn) = tech_turns.last_mut() {
                    if last_turn.first().is_some_and(|first| {
                        first.role == "assistant" && first.tool_calls.is_some()
                    }) || last_turn.last().is_some_and(|last| last.role == "tool")
                    {
                        last_turn.push(tool_msg);
                        continue;
                    }
                }
                tech_turns.push(vec![tool_msg]);
            }
            _ => {}
        }
    }

    let mut tech_messages = vec![ChatMessage::system(system_prompt)];
    let keep_from = tech_turns.len().saturating_sub(max_turns.max(1));
    for turn in tech_turns.into_iter().skip(keep_from) {
        tech_messages.extend(turn);
    }

    (ui_entries, tech_messages)
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

    #[test]
    fn structured_tool_calling_history_roundtrip_and_context_window() {
        let db = Database::open_in_memory().unwrap();
        let mut data = AppData::from_database(db).unwrap();
        let conv_id = "conv-struct-1";

        let u1 = ChatMessage::user("Organizá mis archivos");
        data.record_chat_message(conv_id, Some("Organizá mis archivos"), &u1, MessageKind::Text)
            .unwrap();

        let tc1 = ToolCall {
            id: "call-preview-1".to_string(),
            kind: "function".to_string(),
            function: crate::ai::ToolFunctionCall {
                name: "preview_organize_desktop".to_string(),
                arguments: "{}".to_string(),
            },
            thought_signature: None,
        };
        let a_tc1 = ChatMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![tc1]),
            tool_call_id: None,
            raw_gemini_parts: Some(vec![serde_json::json!({"functionCall":{"name":"preview_organize_desktop","args":{}},"thoughtSignature":"sig-xyz"})]),
        };
        data.record_chat_message(conv_id, None, &a_tc1, MessageKind::ToolCall)
            .unwrap();

        let tr1 = ChatMessage::tool_result("call-preview-1", "25 archivos encontrados");
        data.record_chat_message(conv_id, None, &tr1, MessageKind::ToolResult)
            .unwrap();

        let a1 = ChatMessage::assistant_text("Encontré 25 archivos. ¿Querés organizarlos?");
        data.record_chat_message(conv_id, None, &a1, MessageKind::Text)
            .unwrap();

        // Reload with max_turns = 2 -> drops oldest user turn, keeps atomic tool_call + tool_result together
        let (ui_entries, tech_history) = data
            .load_conversation_history(conv_id, "SYS", 2)
            .unwrap();

        assert_eq!(ui_entries.len(), 2); // User + Assistant text
        assert_eq!(ui_entries[0].text, "Organizá mis archivos");
        assert_eq!(
            ui_entries[1].text,
            "Encontré 25 archivos. ¿Querés organizarlos?"
        );

        // SYS + assistant(tool_call) + tool(result) + assistant(text) = 4 messages
        assert_eq!(tech_history.len(), 4);
        assert_eq!(tech_history[0].role, "system");
        assert_eq!(tech_history[1].role, "assistant");
        assert!(tech_history[1].tool_calls.is_some());
        assert!(tech_history[1].raw_gemini_parts.is_some());
        assert_eq!(tech_history[2].role, "tool");
        assert_eq!(tech_history[2].tool_call_id.as_deref(), Some("call-preview-1"));
        assert_eq!(tech_history[3].role, "assistant");

        let execs = data.db.as_ref().unwrap().list_tool_executions(conv_id).unwrap();
        assert!(execs.is_empty());
    }
}

