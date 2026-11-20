use crate::ai::{ChatMessage, ToolCall};
use crate::persistence::{
    ConversationSummary, Database, FocusSessionRecord, MessageKind, MigrationOutcome,
    NewMusicTrack, NewPersistedMessage, NewToolExecution, PersistedMessage, PersistedPlaylist,
    PersistedPomodoro, PersistedTrack,
};
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PomodoroPrompt {
    WorkFinished,
    BreakFinished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PomodoroTickOutcome {
    WorkFinished(FocusSessionRecord),
    BreakFinished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusStartResult {
    Started,
    ConfirmSwitch { current_label: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomodoroState {
    pub phase: PomodoroPhase,
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub remaining_secs: u32,
    pub running: bool,
    pub completed_sessions: u32,
    pub active_task_id: Option<String>,
    pub active_task_title: Option<String>,
    pub active_session_id: Option<String>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub phase_ref_utc: Option<DateTime<Utc>>,
    pub phase_ref_remaining_secs: u32,
    pub pending_prompt: Option<PomodoroPrompt>,
    #[serde(skip)]
    pub mono_anchor: Option<(Instant, u32)>,
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
            active_task_id: None,
            active_task_title: None,
            active_session_id: None,
            session_started_at: None,
            phase_ref_utc: None,
            phase_ref_remaining_secs: 25 * 60,
            pending_prompt: None,
            mono_anchor: None,
        }
    }
}

impl PomodoroState {
    pub fn start_work(&mut self, custom_minutes: Option<u32>) {
        self.start_work_at(custom_minutes, None, None, Instant::now(), Utc::now());
    }

    pub fn start_work_for_task(
        &mut self,
        custom_minutes: Option<u32>,
        task_id: Option<String>,
        task_title: Option<String>,
    ) {
        self.start_work_at(
            custom_minutes,
            task_id,
            task_title,
            Instant::now(),
            Utc::now(),
        );
    }

    pub fn start_work_at(
        &mut self,
        custom_minutes: Option<u32>,
        task_id: Option<String>,
        task_title: Option<String>,
        now_mono: Instant,
        now_utc: DateTime<Utc>,
    ) {
        if let Some(m) = custom_minutes {
            self.work_minutes = m.clamp(1, 180);
        }
        let total = self.work_minutes * 60;
        self.phase = PomodoroPhase::Work;
        self.remaining_secs = total;
        self.running = true;
        self.active_task_id = task_id;
        self.active_task_title = task_title;
        self.active_session_id = Some(Uuid::new_v4().to_string());
        self.session_started_at = Some(now_utc);
        self.phase_ref_utc = Some(now_utc);
        self.phase_ref_remaining_secs = total;
        self.pending_prompt = None;
        self.mono_anchor = Some((now_mono, total));
    }

    pub fn start_break(&mut self) {
        self.start_break_at(Instant::now(), Utc::now());
    }

    pub fn start_break_at(&mut self, now_mono: Instant, now_utc: DateTime<Utc>) {
        let total = self.break_minutes * 60;
        self.phase = PomodoroPhase::Break;
        self.remaining_secs = total;
        self.running = true;
        self.active_session_id = None;
        self.session_started_at = None;
        self.phase_ref_utc = Some(now_utc);
        self.phase_ref_remaining_secs = total;
        self.pending_prompt = None;
        self.mono_anchor = Some((now_mono, total));
    }

    pub fn toggle_pause(&mut self) {
        self.toggle_pause_at(Instant::now(), Utc::now());
    }

    pub fn toggle_pause_at(&mut self, now_mono: Instant, now_utc: DateTime<Utc>) {
        if self.phase == PomodoroPhase::Idle {
            self.start_work_at(
                None,
                self.active_task_id.clone(),
                self.active_task_title.clone(),
                now_mono,
                now_utc,
            );
        } else if self.running {
            let _ = self.sync_elapsed(now_mono, now_utc);
            self.running = false;
            self.phase_ref_utc = None;
            self.phase_ref_remaining_secs = self.remaining_secs;
            self.mono_anchor = None;
        } else {
            self.running = true;
            self.phase_ref_utc = Some(now_utc);
            self.phase_ref_remaining_secs = self.remaining_secs;
            self.mono_anchor = Some((now_mono, self.remaining_secs));
        }
    }

    pub fn reset(&mut self) {
        self.phase = PomodoroPhase::Idle;
        self.remaining_secs = self.work_minutes * 60;
        self.running = false;
        self.active_session_id = None;
        self.session_started_at = None;
        self.phase_ref_utc = None;
        self.phase_ref_remaining_secs = self.remaining_secs;
        self.pending_prompt = None;
        self.mono_anchor = None;
    }

    /// Builds an `Interrupted` session record if a `Work` session is currently active.
    pub fn build_interrupted_record(&self, now_utc: DateTime<Utc>) -> Option<FocusSessionRecord> {
        if self.phase != PomodoroPhase::Work {
            return None;
        }
        let session_id = self.active_session_id.clone()?;
        let started = self.session_started_at.unwrap_or(now_utc);
        let configured = self.work_minutes * 60;
        let elapsed = configured.saturating_sub(self.remaining_secs);
        Some(FocusSessionRecord {
            id: session_id,
            task_id: self.active_task_id.clone(),
            task_title: self.active_task_title.clone(),
            started_at: started.to_rfc3339(),
            ended_at: now_utc.to_rfc3339(),
            duration_secs: elapsed.max(1),
            status: "Interrupted".to_string(),
        })
    }

    /// Synchronizes remaining seconds using both monotonic `Instant` and UTC wall-clock
    /// (taking `max` so system suspend is accurately accounted for without negative times).
    pub fn sync_elapsed(
        &mut self,
        now_mono: Instant,
        now_utc: DateTime<Utc>,
    ) -> Option<PomodoroTickOutcome> {
        if !self.running || self.phase == PomodoroPhase::Idle {
            return None;
        }

        let mono_elapsed = self
            .mono_anchor
            .map(|(anchor, _)| now_mono.saturating_duration_since(anchor).as_secs() as u32)
            .unwrap_or(0);
        let utc_elapsed = self
            .phase_ref_utc
            .map(|r| now_utc.signed_duration_since(r).num_seconds().max(0) as u32)
            .unwrap_or(0);

        let base_remaining = self
            .mono_anchor
            .map(|(_, rem)| rem)
            .unwrap_or(self.phase_ref_remaining_secs);
        let elapsed = mono_elapsed.max(utc_elapsed);

        if elapsed < base_remaining {
            self.remaining_secs = base_remaining - elapsed;
            return None;
        }

        self.finish_current_phase(now_utc)
    }

    /// Recovers a persisted Pomodoro session on app startup against `now_utc`.
    /// Policy: continues counting while closed unless paused; if expired while closed,
    /// finishes only the active phase without running retroactive break/work loops.
    pub fn recover_on_startup(&mut self, now_utc: DateTime<Utc>) -> Option<PomodoroTickOutcome> {
        if !self.running || self.phase == PomodoroPhase::Idle {
            self.mono_anchor = None;
            return None;
        }
        let Some(ref_utc) = self.phase_ref_utc else {
            self.mono_anchor = Some((Instant::now(), self.remaining_secs));
            self.phase_ref_utc = Some(now_utc);
            self.phase_ref_remaining_secs = self.remaining_secs;
            return None;
        };

        let elapsed = now_utc.signed_duration_since(ref_utc).num_seconds().max(0) as u32;
        if elapsed < self.phase_ref_remaining_secs {
            let rem = self.phase_ref_remaining_secs - elapsed;
            self.remaining_secs = rem;
            self.phase_ref_utc = Some(now_utc);
            self.phase_ref_remaining_secs = rem;
            self.mono_anchor = Some((Instant::now(), rem));
            None
        } else {
            self.finish_current_phase(now_utc)
        }
    }

    fn finish_current_phase(&mut self, now_utc: DateTime<Utc>) -> Option<PomodoroTickOutcome> {
        match self.phase {
            PomodoroPhase::Work => {
                self.completed_sessions += 1;
                let session_id = self
                    .active_session_id
                    .take()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                let started = self.session_started_at.take().unwrap_or(now_utc);
                let configured = self.work_minutes * 60;
                let record = FocusSessionRecord {
                    id: session_id,
                    task_id: self.active_task_id.clone(),
                    task_title: self.active_task_title.clone(),
                    started_at: started.to_rfc3339(),
                    ended_at: now_utc.to_rfc3339(),
                    duration_secs: configured,
                    status: "Completed".to_string(),
                };
                self.phase = PomodoroPhase::Idle;
                self.remaining_secs = 0;
                self.running = false;
                self.phase_ref_utc = None;
                self.phase_ref_remaining_secs = 0;
                self.mono_anchor = None;
                self.pending_prompt = Some(PomodoroPrompt::WorkFinished);
                Some(PomodoroTickOutcome::WorkFinished(record))
            }
            PomodoroPhase::Break => {
                self.phase = PomodoroPhase::Idle;
                self.remaining_secs = self.work_minutes * 60;
                self.running = false;
                self.phase_ref_utc = None;
                self.phase_ref_remaining_secs = self.remaining_secs;
                self.mono_anchor = None;
                self.pending_prompt = Some(PomodoroPrompt::BreakFinished);
                Some(PomodoroTickOutcome::BreakFinished)
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
    pub playlists: Vec<PersistedPlaylist>,
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
            playlists: vec![],
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

fn prompt_to_str(p: PomodoroPrompt) -> &'static str {
    match p {
        PomodoroPrompt::WorkFinished => "WorkFinished",
        PomodoroPrompt::BreakFinished => "BreakFinished",
    }
}

fn str_to_prompt(s: &str) -> Option<PomodoroPrompt> {
    match s {
        "WorkFinished" => Some(PomodoroPrompt::WorkFinished),
        "BreakFinished" => Some(PomodoroPrompt::BreakFinished),
        _ => None,
    }
}

fn parse_rfc3339_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn pomodoro_to_persisted(p: &PomodoroState) -> PersistedPomodoro {
    PersistedPomodoro {
        phase: phase_to_str(p.phase).to_string(),
        work_minutes: p.work_minutes,
        break_minutes: p.break_minutes,
        remaining_secs: p.remaining_secs,
        completed_sessions: p.completed_sessions,
        active_task_id: p.active_task_id.clone(),
        active_task_title: p.active_task_title.clone(),
        running: p.running,
        phase_ref_utc: p.phase_ref_utc.map(|d| d.to_rfc3339()),
        phase_ref_remaining_secs: p.phase_ref_remaining_secs,
        active_session_id: p.active_session_id.clone(),
        session_started_at: p.session_started_at.map(|d| d.to_rfc3339()),
        pending_prompt: p.pending_prompt.map(|pr| prompt_to_str(pr).to_string()),
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
                Err(e) => Self {
                    startup_error: Some(format!("Error cargando SQLite: {e}")),
                    ..Self::default()
                },
            },
            Err(e) => Self {
                startup_error: Some(format!("Error abriendo/migrando SQLite: {e}")),
                ..Self::default()
            },
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
                db.upsert_habit(
                    &h.id,
                    &h.name,
                    &h.icon,
                    h.daily_goal,
                    h.current_count,
                    h.last_date,
                )?;
            }
            habits = defaults;
        }

        let mut pomodoro = match db.load_pomodoro()? {
            Some(p) => PomodoroState {
                phase: str_to_phase(&p.phase),
                work_minutes: p.work_minutes,
                break_minutes: p.break_minutes,
                remaining_secs: p.remaining_secs,
                running: p.running,
                completed_sessions: p.completed_sessions,
                active_task_id: p.active_task_id,
                active_task_title: p.active_task_title,
                active_session_id: p.active_session_id,
                session_started_at: p.session_started_at.as_deref().and_then(parse_rfc3339_utc),
                phase_ref_utc: p.phase_ref_utc.as_deref().and_then(parse_rfc3339_utc),
                phase_ref_remaining_secs: p.phase_ref_remaining_secs,
                pending_prompt: p.pending_prompt.as_deref().and_then(str_to_prompt),
                mono_anchor: None,
            },
            None => {
                let def = PomodoroState::default();
                db.save_pomodoro(&pomodoro_to_persisted(&def))?;
                def
            }
        };

        if let Some(outcome) = pomodoro.recover_on_startup(Utc::now()) {
            if let PomodoroTickOutcome::WorkFinished(record) = outcome {
                db.record_focus_session(&record)?;
            }
            db.save_pomodoro(&pomodoro_to_persisted(&pomodoro))?;
        } else if pomodoro.running {
            db.save_pomodoro(&pomodoro_to_persisted(&pomodoro))?;
        }

        let playlists = db.list_playlists()?;

        Ok(Self {
            tasks,
            habits,
            pomodoro,
            playlists,
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
            db.upsert_habit(
                &h.id,
                &h.name,
                &h.icon,
                h.daily_goal,
                h.current_count,
                h.last_date,
            )?;
        }
        db.save_pomodoro(&pomodoro_to_persisted(&self.pomodoro))?;
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
        if self.pomodoro.active_task_id.as_deref() == Some(id.as_str()) {
            self.pomodoro.active_task_id = None;
            self.pomodoro.active_task_title = None;
            let _ = self.save_pomodoro();
        }
        Ok(())
    }

    /// Renames a task if `new_title` is non-empty after trimming.
    pub fn rename_task(&mut self, task_id: &str, new_title: &str) -> anyhow::Result<bool> {
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) else {
            return Ok(false);
        };
        task.title = trimmed.to_string();
        let snapshot = task.clone();
        if self.pomodoro.active_task_id.as_deref() == Some(task_id) {
            self.pomodoro.active_task_title = Some(trimmed.to_string());
            let _ = self.save_pomodoro();
        }
        if self.db.is_some() {
            self.upsert_task(&snapshot)?;
        }
        Ok(true)
    }

    /// Soft-deletes all completed tasks while preserving historical `focus_sessions`.
    pub fn clear_completed_tasks(&mut self) -> anyhow::Result<usize> {
        let removed = if let Some(db) = &self.db {
            db.delete_completed_tasks()?
        } else {
            self.tasks.iter().filter(|t| t.done).count()
        };
        self.tasks.retain(|t| !t.done);
        Ok(removed)
    }

    /// Returns the live title of the currently focused task (if it still exists in `tasks`).
    pub fn active_task_display_title(&self) -> Option<String> {
        let id = self.pomodoro.active_task_id.as_deref()?;
        self.tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
    }

    /// Starts focusing on `task_id`. If another session is active and `!force_switch`,
    /// returns `ConfirmSwitch` without modifying timer state.
    pub fn focus_on_task(&mut self, task_id: &str, force_switch: bool) -> FocusStartResult {
        let Some(task) = self.tasks.iter().find(|t| t.id == task_id).cloned() else {
            return FocusStartResult::Started;
        };
        let has_active_session = self.pomodoro.phase != PomodoroPhase::Idle;
        let same_task = self.pomodoro.active_task_id.as_deref() == Some(task_id);
        if has_active_session && !same_task && !force_switch {
            let current_label = self
                .active_task_display_title()
                .or_else(|| self.pomodoro.active_task_title.clone())
                .unwrap_or_else(|| "otra sesión".to_string());
            return FocusStartResult::ConfirmSwitch { current_label };
        }

        if has_active_session && !same_task {
            if let Some(interrupted) = self.pomodoro.build_interrupted_record(Utc::now()) {
                if let Some(db) = &self.db {
                    let _ = db.record_focus_session(&interrupted);
                }
            }
        }

        self.pomodoro
            .start_work_for_task(None, Some(task.id), Some(task.title));
        let _ = self.save_pomodoro();
        FocusStartResult::Started
    }

    pub fn sync_pomodoro_elapsed(&mut self) -> Option<PomodoroTickOutcome> {
        let outcome = self.pomodoro.sync_elapsed(Instant::now(), Utc::now())?;
        if let PomodoroTickOutcome::WorkFinished(ref record) = outcome {
            if let Some(db) = &self.db {
                let _ = db.record_focus_session(record);
            }
        }
        let _ = self.save_pomodoro();
        Some(outcome)
    }

    pub fn toggle_pomodoro_and_persist(&mut self) {
        self.pomodoro.toggle_pause();
        let _ = self.save_pomodoro();
    }

    pub fn reset_pomodoro_and_persist(&mut self) {
        if let Some(interrupted) = self.pomodoro.build_interrupted_record(Utc::now()) {
            if let Some(db) = &self.db {
                let _ = db.record_focus_session(&interrupted);
            }
        }
        self.pomodoro.reset();
        let _ = self.save_pomodoro();
    }

    pub fn start_break_and_persist(&mut self) {
        self.pomodoro.start_break();
        let _ = self.save_pomodoro();
    }

    /// Marks the currently focused task as done, clears prompt/active link, and persists.
    pub fn complete_focused_task(&mut self) -> Option<String> {
        let task_id = self.pomodoro.active_task_id.take();
        self.pomodoro.active_task_title = None;
        self.pomodoro.pending_prompt = None;
        self.pomodoro.remaining_secs = self.pomodoro.work_minutes * 60;
        self.pomodoro.phase_ref_remaining_secs = self.pomodoro.remaining_secs;

        let mut completed_title = None;
        if let Some(id) = task_id {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.done = true;
                completed_title = Some(task.title.clone());
                let snapshot = task.clone();
                if self.db.is_some() {
                    let _ = self.upsert_task(&snapshot);
                }
            }
        }
        let _ = self.save_pomodoro();
        completed_title
    }

    /// Starts another work block for the same associated task (even if skipping a break).
    pub fn continue_focused_task(&mut self) {
        if self.pomodoro.phase == PomodoroPhase::Work {
            if let Some(interrupted) = self.pomodoro.build_interrupted_record(Utc::now()) {
                if let Some(db) = &self.db {
                    let _ = db.record_focus_session(&interrupted);
                }
            }
        }
        let tid = self.pomodoro.active_task_id.clone();
        let ttitle = self
            .active_task_display_title()
            .or_else(|| self.pomodoro.active_task_title.clone());
        self.pomodoro.start_work_for_task(None, tid, ttitle);
        let _ = self.save_pomodoro();
    }

    pub fn dismiss_pomodoro_prompt(&mut self, clear_task: bool) {
        self.pomodoro.pending_prompt = None;
        if clear_task {
            self.pomodoro.active_task_id = None;
            self.pomodoro.active_task_title = None;
        }
        self.pomodoro.remaining_secs = self.pomodoro.work_minutes * 60;
        self.pomodoro.phase_ref_remaining_secs = self.pomodoro.remaining_secs;
        let _ = self.save_pomodoro();
    }

    /// Returns `(completed_sessions_today, total_focus_minutes_today)`.
    pub fn today_focus_summary(&self) -> (u32, u64) {
        let Some(db) = &self.db else {
            return (self.pomodoro.completed_sessions, 0);
        };
        let today = Local::now().date_naive();
        let start_local = today
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
            .unwrap_or_default();
        let end_local = today
            .succ_opt()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
            .unwrap_or_default();
        match db.focus_summary_in_range(&start_local, &end_local) {
            Ok((count, secs)) => (count, secs / 60),
            Err(_) => (self.pomodoro.completed_sessions, 0),
        }
    }

    pub fn import_m3u_playlist(
        &mut self,
        name: &str,
        source_path: Option<&str>,
        tracks: &[NewMusicTrack],
    ) -> anyhow::Result<PersistedPlaylist> {
        let Some(db) = self.db.as_mut() else {
            anyhow::bail!("Base de datos SQLite no disponible.");
        };
        let playlist = db.save_playlist_with_tracks(name, source_path, tracks)?;
        self.playlists.insert(0, playlist.clone());
        Ok(playlist)
    }

    pub fn load_playlist_tracks(&self, playlist_id: &str) -> anyhow::Result<Vec<PersistedTrack>> {
        let Some(db) = self.db.as_ref() else {
            return Ok(vec![]);
        };
        db.list_tracks(playlist_id)
    }

    pub fn add_music_url(
        &mut self,
        target_playlist_id: Option<&str>,
        title: Option<&str>,
        url: &str,
    ) -> anyhow::Result<(PersistedPlaylist, PersistedTrack)> {
        let normalized_url =
            crate::music::normalize_audio_url(url).map_err(|e| anyhow::anyhow!(e))?;
        let track_title = title
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| crate::music::infer_title_from_uri(&normalized_url));

        let Some(db) = self.db.as_mut() else {
            anyhow::bail!("Base de datos SQLite no disponible.");
        };

        let existing_id = target_playlist_id
            .filter(|pid| self.playlists.iter().any(|p| p.id == *pid))
            .map(str::to_string)
            .or_else(|| {
                self.playlists
                    .iter()
                    .find(|p| p.name == "URLs guardadas")
                    .map(|p| p.id.clone())
            });

        if let Some(pid) = existing_id {
            let track = db.add_track_to_playlist(&pid, &track_title, &normalized_url, None)?;
            let mut updated_playlist = None;
            if let Some(pl) = self.playlists.iter_mut().find(|p| p.id == pid) {
                pl.track_count += 1;
                updated_playlist = Some(pl.clone());
            }
            let playlist = updated_playlist
                .ok_or_else(|| anyhow::anyhow!("Playlist no encontrada en memoria."))?;
            return Ok((playlist, track));
        }

        let new_track = NewMusicTrack {
            title: track_title,
            uri: normalized_url,
            duration_secs: None,
        };
        let playlist = db.save_playlist_with_tracks("URLs guardadas", None, &[new_track])?;
        let tracks = db.list_tracks(&playlist.id)?;
        let track = tracks
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No se pudo recuperar la pista guardada."))?;
        self.playlists.insert(0, playlist.clone());
        Ok((playlist, track))
    }

    pub fn delete_music_track(&mut self, playlist_id: &str, track_id: &str) -> anyhow::Result<()> {
        if let Some(db) = self.db.as_ref() {
            db.delete_track(track_id)?;
        }
        if let Some(pl) = self.playlists.iter_mut().find(|p| p.id == playlist_id) {
            pl.track_count = pl.track_count.saturating_sub(1);
        }
        Ok(())
    }

    pub fn delete_playlist(&mut self, playlist_id: &str) -> anyhow::Result<()> {
        if let Some(db) = self.db.as_ref() {
            db.delete_playlist(playlist_id)?;
        }
        self.playlists.retain(|p| p.id != playlist_id);
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

    /// Saves one explicit memory when the user asked to remember something.
    /// Does not advance Dreaming's cursor and refuses obvious secrets.
    pub fn save_explicit_memory(&self, content: &str) -> anyhow::Result<()> {
        let trimmed = content.trim();
        let len = trimmed.chars().count();
        if !(3..=400).contains(&len) {
            anyhow::bail!("El recuerdo debe tener entre 3 y 400 caracteres.");
        }
        let lower = trimmed.to_lowercase();
        const BLOCKED: &[&str] = &[
            "password",
            "contraseña",
            "contrasena",
            "api key",
            "api_key",
            "apikey",
            "secret",
            "token",
        ];
        if BLOCKED.iter().any(|needle| lower.contains(needle)) {
            anyhow::bail!("No guardo contraseñas, tokens ni secretos en la memoria.");
        }
        let Some(db) = &self.db else {
            anyhow::bail!("Base de datos SQLite no inicializada");
        };
        if !db.dreaming_insert_memory(trimmed)? {
            anyhow::bail!("Ese recuerdo ya estaba guardado. No lo dupliqué.");
        }
        Ok(())
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
        db.save_pomodoro(&pomodoro_to_persisted(&self.pomodoro))
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
        include_pending: bool,
    ) -> anyhow::Result<bool> {
        let Some(db) = &self.db else {
            return Ok(false);
        };
        db.has_tool_execution(conversation_id, tool_call_id, include_pending)
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
        let t0_mono = Instant::now();
        let t0_utc = Utc::now();
        p.start_work_at(Some(1), None, None, t0_mono, t0_utc);
        assert_eq!(p.phase, PomodoroPhase::Work);
        assert!(p.running);

        // 20s elapsed -> 40s remaining
        assert!(p
            .sync_elapsed(
                t0_mono + std::time::Duration::from_secs(20),
                t0_utc + chrono::Duration::seconds(20)
            )
            .is_none());
        assert_eq!(p.remaining_secs, 40);

        // Full 60s elapsed -> WorkFinished prompt, does NOT auto-start Break
        let out = p.sync_elapsed(
            t0_mono + std::time::Duration::from_secs(60),
            t0_utc + chrono::Duration::seconds(60),
        );
        assert!(matches!(out, Some(PomodoroTickOutcome::WorkFinished(_))));
        assert_eq!(p.completed_sessions, 1);
        assert_eq!(p.phase, PomodoroPhase::Idle);
        assert!(!p.running);
        assert_eq!(p.pending_prompt, Some(PomodoroPrompt::WorkFinished));

        // User starts break -> 60s -> BreakFinished prompt
        let t1_mono = t0_mono + std::time::Duration::from_secs(65);
        let t1_utc = t0_utc + chrono::Duration::seconds(65);
        p.start_break_at(t1_mono, t1_utc);
        assert_eq!(p.phase, PomodoroPhase::Break);
        let bout = p.sync_elapsed(
            t1_mono + std::time::Duration::from_secs(60),
            t1_utc + chrono::Duration::seconds(60),
        );
        assert_eq!(bout, Some(PomodoroTickOutcome::BreakFinished));
        assert_eq!(p.phase, PomodoroPhase::Idle);
        assert!(!p.running);
        assert_eq!(p.pending_prompt, Some(PomodoroPrompt::BreakFinished));
    }

    #[test]
    fn structured_tool_calling_history_roundtrip_and_context_window() {
        let db = Database::open_in_memory().unwrap();
        let mut data = AppData::from_database(db).unwrap();
        let conv_id = "conv-struct-1";

        let u1 = ChatMessage::user("Organizá mis archivos");
        data.record_chat_message(
            conv_id,
            Some("Organizá mis archivos"),
            &u1,
            MessageKind::Text,
        )
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
            raw_gemini_parts: Some(vec![
                serde_json::json!({"functionCall":{"name":"preview_organize_desktop","args":{}},"thoughtSignature":"sig-xyz"}),
            ]),
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
        let (ui_entries, tech_history) = data.load_conversation_history(conv_id, "SYS", 2).unwrap();

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
        assert_eq!(
            tech_history[2].tool_call_id.as_deref(),
            Some("call-preview-1")
        );
        assert_eq!(tech_history[3].role, "assistant");

        let execs = data
            .db
            .as_ref()
            .unwrap()
            .list_tool_executions(conv_id)
            .unwrap();
        assert!(execs.is_empty());
    }

    #[test]
    fn pomodoro_pause_resume_and_system_suspend_handling() {
        let mut p = PomodoroState {
            work_minutes: 10,
            break_minutes: 5,
            ..Default::default()
        };
        let t0_mono = Instant::now();
        let t0_utc = Utc::now();
        p.start_work_at(Some(10), None, None, t0_mono, t0_utc);

        // Advance 30s, then pause
        let t1_mono = t0_mono + std::time::Duration::from_secs(30);
        let t1_utc = t0_utc + chrono::Duration::seconds(30);
        p.toggle_pause_at(t1_mono, t1_utc);
        assert!(!p.running);
        assert_eq!(p.remaining_secs, 570);

        // Time passes while paused -> remaining_secs does not decrease
        assert!(p
            .sync_elapsed(
                t1_mono + std::time::Duration::from_secs(120),
                t1_utc + chrono::Duration::seconds(120)
            )
            .is_none());
        assert_eq!(p.remaining_secs, 570);

        // Resume, then simulate system suspend (Instant advances 0s, UTC jumps +200s)
        let t2_mono = t1_mono + std::time::Duration::from_secs(120);
        let t2_utc = t1_utc + chrono::Duration::seconds(120);
        p.toggle_pause_at(t2_mono, t2_utc);
        assert!(p.running);
        assert!(p
            .sync_elapsed(t2_mono, t2_utc + chrono::Duration::seconds(200))
            .is_none());
        assert_eq!(p.remaining_secs, 370);
    }

    #[test]
    fn pomodoro_restart_recovery_policy_running_paused_and_expired() {
        let now = Utc::now();

        // 1. Running session recovers remaining time
        let mut running_p = PomodoroState::default();
        running_p.start_work_at(
            Some(25),
            Some("task-1".into()),
            Some("Test".into()),
            Instant::now(),
            now - chrono::Duration::seconds(600),
        );
        assert!(running_p.recover_on_startup(now).is_none());
        assert!(running_p.running);
        assert_eq!(running_p.remaining_secs, 900);

        // 2. Paused session stays paused at exact remaining_secs
        let mut paused_p = PomodoroState::default();
        paused_p.start_work_at(
            Some(25),
            None,
            None,
            Instant::now(),
            now - chrono::Duration::seconds(60),
        );
        paused_p.toggle_pause_at(Instant::now(), now);
        let rem_before = paused_p.remaining_secs;
        assert!(paused_p
            .recover_on_startup(now + chrono::Duration::hours(2))
            .is_none());
        assert!(!paused_p.running);
        assert_eq!(paused_p.remaining_secs, rem_before);

        // 3. Expired while closed -> finishes single Work phase, does NOT auto-run break loop
        let mut expired_p = PomodoroState::default();
        expired_p.start_work_at(
            Some(1),
            Some("task-1".into()),
            Some("Test".into()),
            Instant::now(),
            now - chrono::Duration::hours(3),
        );
        let outcome = expired_p.recover_on_startup(now);
        assert!(matches!(
            outcome,
            Some(PomodoroTickOutcome::WorkFinished(_))
        ));
        assert_eq!(expired_p.completed_sessions, 1);
        assert_eq!(expired_p.phase, PomodoroPhase::Idle);
        assert!(!expired_p.running);
        assert_eq!(expired_p.pending_prompt, Some(PomodoroPrompt::WorkFinished));
    }

    #[test]
    fn task_focus_association_switch_delete_rename_and_session_dedup() {
        let db = Database::open_in_memory().unwrap();
        let mut data = AppData::from_database(db).unwrap();

        let t1 = TaskItem::new("Revisar PR de Rusty");
        let t2 = TaskItem::new("Escribir tests");
        data.upsert_task(&t1).unwrap();
        data.upsert_task(&t2).unwrap();
        data.tasks.push(t1.clone());
        data.tasks.push(t2.clone());

        // Start focus on t1
        assert_eq!(data.focus_on_task(&t1.id, false), FocusStartResult::Started);
        assert_eq!(data.pomodoro.phase, PomodoroPhase::Work);
        assert_eq!(
            data.active_task_display_title().as_deref(),
            Some("Revisar PR de Rusty")
        );

        // Attempting to focus t2 without force asks for confirmation
        assert_eq!(
            data.focus_on_task(&t2.id, false),
            FocusStartResult::ConfirmSwitch {
                current_label: "Revisar PR de Rusty".to_string()
            }
        );
        assert_eq!(
            data.pomodoro.active_task_id.as_deref(),
            Some(t1.id.as_str())
        );

        // Force switch records Interrupted for t1 and starts t2
        assert_eq!(data.focus_on_task(&t2.id, true), FocusStartResult::Started);
        assert_eq!(
            data.pomodoro.active_task_id.as_deref(),
            Some(t2.id.as_str())
        );
        let sessions = data.db.as_ref().unwrap().list_focus_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, "Interrupted");
        assert_eq!(sessions[0].task_id.as_deref(), Some(t1.id.as_str()));

        // Rename t2 updates task and active_task_title, rejects empty
        assert!(!data.rename_task(&t2.id, "   ").unwrap());
        assert!(data.rename_task(&t2.id, "Escribir tests v0.2.2").unwrap());
        assert_eq!(
            data.active_task_display_title().as_deref(),
            Some("Escribir tests v0.2.2")
        );

        // Complete t2's session and mark completed
        let record = FocusSessionRecord {
            id: "sess-dedup-1".to_string(),
            task_id: Some(t2.id.clone()),
            task_title: Some("Escribir tests v0.2.2".to_string()),
            started_at: Utc::now().to_rfc3339(),
            ended_at: Utc::now().to_rfc3339(),
            duration_secs: 1500,
            status: "Completed".to_string(),
        };
        data.db
            .as_ref()
            .unwrap()
            .record_focus_session(&record)
            .unwrap();
        data.db
            .as_ref()
            .unwrap()
            .record_focus_session(&record)
            .unwrap(); // idempotent!
        let (today_count, today_mins) = data.today_focus_summary();
        assert_eq!(today_count, 1);
        assert_eq!(today_mins, 25);

        let completed = data.complete_focused_task();
        assert_eq!(completed.as_deref(), Some("Escribir tests v0.2.2"));
        assert!(data.tasks.iter().find(|t| t.id == t2.id).unwrap().done);

        // Clear completed tasks preserves historical focus_sessions
        assert_eq!(data.clear_completed_tasks().unwrap(), 1);
        assert_eq!(data.tasks.len(), 1);
        let sessions_after = data.db.as_ref().unwrap().list_focus_sessions().unwrap();
        assert_eq!(sessions_after.len(), 2);

        // Delete remaining task while focused does not error and clears active link safely
        data.focus_on_task(&t1.id, true);
        data.delete_task_at(0).unwrap();
        assert_eq!(data.active_task_display_title(), None);
    }

    #[test]
    fn clock_format_habit_rollover_and_phase_codes_are_stable() {
        let mut timer = PomodoroState {
            remaining_secs: 125,
            ..PomodoroState::default()
        };
        assert_eq!(timer.formatted_time(), "02:05");
        timer.remaining_secs = 0;
        assert_eq!(timer.formatted_time(), "00:00");
        for phase in [
            PomodoroPhase::Idle,
            PomodoroPhase::Work,
            PomodoroPhase::Break,
        ] {
            assert_eq!(str_to_phase(phase_to_str(phase)), phase);
        }
        assert_eq!(str_to_phase("nope"), PomodoroPhase::Idle);
        assert_eq!(
            str_to_prompt("WorkFinished"),
            Some(PomodoroPrompt::WorkFinished)
        );
        assert_eq!(str_to_prompt(""), None);
        assert_eq!(format_local_datetime_from_utc("not-a-date"), "not-a-date");

        let mut habit = HabitItem::new("Agua", "💧", 0);
        assert_eq!(habit.daily_goal, 1);
        habit.current_count = 4;
        habit.last_date = habit.last_date.pred_opt().unwrap();
        habit.ensure_today();
        assert_eq!(habit.current_count, 0);
        assert_eq!(habit.last_date, chrono::Local::now().date_naive());
    }
}
