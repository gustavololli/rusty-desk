use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    AlreadyCompleted,
    NoLegacyFile,
    EmptyFile,
    Migrated {
        tasks: usize,
        habits: usize,
        conversations: usize,
        messages: usize,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyAppData {
    tasks: Vec<LegacyTaskItem>,
    habits: Vec<LegacyHabitItem>,
    pomodoro: LegacyPomodoroState,
    chat_sessions: Vec<LegacyChatSession>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyTaskItem {
    id: String,
    title: String,
    done: bool,
    created_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyHabitItem {
    id: String,
    name: String,
    icon: String,
    daily_goal: u32,
    current_count: u32,
    last_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct LegacyPomodoroState {
    phase: String,
    work_minutes: u32,
    break_minutes: u32,
    remaining_secs: u32,
    completed_sessions: u32,
}

impl Default for LegacyPomodoroState {
    fn default() -> Self {
        Self {
            phase: "Idle".to_string(),
            work_minutes: 25,
            break_minutes: 5,
            remaining_secs: 25 * 60,
            completed_sessions: 0,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyChatSession {
    id: String,
    title: String,
    updated_at: String,
    ui_entries: Vec<LegacyUiChatEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyUiChatEntry {
    is_user: bool,
    text: String,
    timestamp: String,
}

fn normalize_utc_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Utc::now().to_rfc3339();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M") {
        if let Some(local_dt) = Local.from_local_datetime(&naive).earliest() {
            return local_dt.with_timezone(&Utc).to_rfc3339();
        }
    }
    Utc::now().to_rfc3339()
}

pub fn migrate_json_if_needed(
    conn: &mut Connection,
    json_path: &Path,
) -> anyhow::Result<MigrationOutcome> {
    let meta_status: Option<String> = conn
        .query_row(
            "SELECT value FROM app_meta WHERE key = 'json_migration_v1'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if matches!(
        meta_status.as_deref(),
        Some("completed") | Some("completed_empty")
    ) {
        return Ok(MigrationOutcome::AlreadyCompleted);
    }

    if !json_path.exists() {
        return Ok(MigrationOutcome::NoLegacyFile);
    }

    let raw = fs::read_to_string(json_path).map_err(|e| {
        anyhow::anyhow!(
            "No se pudo leer '{}' para migración: {e}",
            json_path.display()
        )
    })?;

    if raw.trim().is_empty() {
        conn.execute(
            "INSERT OR REPLACE INTO app_meta (key, value, updated_at) VALUES ('json_migration_v1', 'completed_empty', ?1)",
            [Utc::now().to_rfc3339()],
        )?;
        return Ok(MigrationOutcome::EmptyFile);
    }

    let legacy: LegacyAppData = serde_json::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "El archivo '{}' está corrupto o es inválido y no pudo migrarse: {e}",
            json_path.display()
        )
    })?;

    // Create a recoverable backup before mutating SQLite; never delete the original data.json.
    let backup_path = json_path.with_extension("json.bak");
    if !backup_path.exists() {
        fs::copy(json_path, &backup_path).map_err(|e| {
            anyhow::anyhow!(
                "No se pudo crear respaldo '{}' antes de migrar: {e}",
                backup_path.display()
            )
        })?;
    }

    let now_utc = Utc::now().to_rfc3339();
    let today_str = Local::now().date_naive().to_string();

    let tx = conn.transaction()?;

    let mut imported_tasks = 0usize;
    for t in &legacy.tasks {
        let id = if t.id.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            t.id.clone()
        };
        let title = if t.title.trim().is_empty() {
            "Tarea sin título"
        } else {
            t.title.trim()
        };
        let created_utc = normalize_utc_timestamp(&t.created_at);
        tx.execute(
            "INSERT OR IGNORE INTO tasks (id, title, done, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 0)",
            params![id, title, if t.done { 1 } else { 0 }, created_utc, now_utc],
        )?;
        imported_tasks += 1;
    }

    let mut imported_habits = 0usize;
    for h in &legacy.habits {
        let id = if h.id.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            h.id.clone()
        };
        let name = if h.name.trim().is_empty() {
            "Hábito"
        } else {
            h.name.trim()
        };
        let icon = if h.icon.trim().is_empty() {
            "✨"
        } else {
            h.icon.trim()
        };
        let goal = h.daily_goal.max(1);
        let last_date = h
            .last_date
            .map(|d| d.to_string())
            .unwrap_or_else(|| today_str.clone());

        let inserted = tx.execute(
            "INSERT OR IGNORE INTO habits (id, name, icon, daily_goal, current_count, last_date, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0)",
            params![id, name, icon, goal, h.current_count, last_date, now_utc, now_utc],
        )?;
        if inserted > 0 && h.current_count > 0 {
            tx.execute(
                "INSERT INTO habit_entries (id, habit_id, entry_date, delta, created_at, updated_at, version, dirty)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0)",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    last_date,
                    h.current_count as i64,
                    now_utc,
                    now_utc
                ],
            )?;
        }
        imported_habits += 1;
    }

    tx.execute(
        "INSERT OR IGNORE INTO pomodoro_state (id, phase, work_minutes, break_minutes, remaining_secs, completed_sessions, updated_at, version, dirty)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 1, 0)",
        params![
            legacy.pomodoro.phase,
            legacy.pomodoro.work_minutes.clamp(1, 180),
            legacy.pomodoro.break_minutes.clamp(1, 60),
            legacy.pomodoro.remaining_secs,
            legacy.pomodoro.completed_sessions,
            now_utc
        ],
    )?;

    let mut imported_conversations = 0usize;
    let mut imported_messages = 0usize;
    for session in &legacy.chat_sessions {
        let conv_id = if session.id.trim().is_empty() {
            Uuid::new_v4().to_string()
        } else {
            session.id.clone()
        };
        let title = if session.title.trim().is_empty() {
            "Chat migrado"
        } else {
            session.title.trim()
        };
        let conv_updated = normalize_utc_timestamp(&session.updated_at);

        let conv_inserted = tx.execute(
            "INSERT OR IGNORE INTO conversations (id, title, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, 1, 0)",
            params![conv_id, title, conv_updated, conv_updated],
        )?;
        imported_conversations += 1;

        if conv_inserted > 0 {
            for (seq, entry) in session.ui_entries.iter().enumerate() {
                let role = if entry.is_user { "user" } else { "assistant" };
                tx.execute(
                    "INSERT INTO messages (id, conversation_id, seq, role, kind, content, created_at, updated_at, version, dirty)
                     VALUES (?1, ?2, ?3, ?4, 'text', ?5, ?6, ?7, 1, 0)",
                    params![
                        Uuid::new_v4().to_string(),
                        conv_id,
                        seq as i64,
                        role,
                        entry.text,
                        conv_updated,
                        conv_updated
                    ],
                )?;
                imported_messages += 1;
            }
        }
    }

    // Validate counts and referential integrity before marking migration completed.
    let db_tasks: usize = tx.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
    let db_habits: usize = tx.query_row("SELECT COUNT(*) FROM habits", [], |r| r.get(0))?;
    let db_convs: usize = tx.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
    if db_tasks < imported_tasks || db_habits < imported_habits || db_convs < imported_conversations
    {
        anyhow::bail!("Validación de migración fallida: conteo inconsistente de registros.");
    }

    tx.execute(
        "INSERT OR REPLACE INTO app_meta (key, value, updated_at) VALUES ('json_migration_v1', 'completed', ?1)",
        [now_utc],
    )?;

    tx.commit()?;

    Ok(MigrationOutcome::Migrated {
        tasks: imported_tasks,
        habits: imported_habits,
        conversations: imported_conversations,
        messages: imported_messages,
    })
}
