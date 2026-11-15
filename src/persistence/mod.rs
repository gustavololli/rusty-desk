pub mod json_migration;
pub mod migrations;

use chrono::{NaiveDate, Utc};
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub use json_migration::{migrate_json_if_needed, MigrationOutcome};

#[derive(Debug, Clone)]
pub struct PersistedTask {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PersistedHabit {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub daily_goal: u32,
    pub current_count: u32,
    pub last_date: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct PersistedPomodoro {
    pub phase: String,
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub remaining_secs: u32,
    pub completed_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    ToolCall,
    ToolResult,
    ToolError,
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::ToolError => "tool_error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "tool_call" => Self::ToolCall,
            "tool_result" => Self::ToolResult,
            "tool_error" => Self::ToolError,
            _ => Self::Text,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PersistedMessage {
    pub id: String,
    pub conversation_id: String,
    pub seq: i64,
    pub role: String,
    pub kind: MessageKind,
    pub content: Option<String>,
    pub tool_calls_json: Option<String>,
    pub tool_call_id: Option<String>,
    pub raw_gemini_parts_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewPersistedMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub kind: MessageKind,
    pub content: Option<String>,
    pub tool_calls_json: Option<String>,
    pub tool_call_id: Option<String>,
    pub raw_gemini_parts_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewToolExecution {
    pub id: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: String,
    pub source: String,
    pub arguments_json: String,
    pub status: String,
    pub result_summary: Option<String>,
    pub result_full: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PersistedToolExecution {
    pub id: String,
    pub conversation_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: String,
    pub source: String,
    pub arguments_json: String,
    pub status: String,
    pub result_summary: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn default_db_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
            proj.data_dir().join("rusty_desk.db")
        } else {
            PathBuf::from("rusty_desk.db")
        }
    }

    pub fn open(db_path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn open_with_json_migration(
        db_path: &Path,
        json_path: &Path,
    ) -> anyhow::Result<(Self, MigrationOutcome)> {
        let mut db = Self::open(db_path)?;
        let outcome = migrate_json_if_needed(&mut db.conn, json_path)?;
        Ok((db, outcome))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::run_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<PersistedTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, done, created_at
             FROM tasks
             WHERE deleted_at IS NULL
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let done_int: i64 = row.get(2)?;
            Ok(PersistedTask {
                id: row.get(0)?,
                title: row.get(1)?,
                done: done_int != 0,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_task(
        &self,
        id: &str,
        title: &str,
        done: bool,
        created_at_utc: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO tasks (id, title, done, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 1)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                done = excluded.done,
                updated_at = excluded.updated_at,
                version = tasks.version + 1,
                dirty = 1",
            params![id, title, if done { 1 } else { 0 }, created_at_utc, now],
        )?;
        Ok(())
    }

    pub fn soft_delete_task(&self, id: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tasks SET deleted_at = ?2, updated_at = ?2, version = version + 1, dirty = 1 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn list_habits(&self) -> anyhow::Result<Vec<PersistedHabit>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, icon, daily_goal, current_count, last_date
             FROM habits
             WHERE deleted_at IS NULL
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let date_str: String = row.get(5)?;
            let last_date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .unwrap_or_else(|_| chrono::Local::now().date_naive());
            Ok(PersistedHabit {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
                daily_goal: row.get::<_, u32>(3)?.max(1),
                current_count: row.get(4)?,
                last_date,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_habit(
        &self,
        id: &str,
        name: &str,
        icon: &str,
        daily_goal: u32,
        current_count: u32,
        last_date: NaiveDate,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO habits (id, name, icon, daily_goal, current_count, last_date, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, 1)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                icon = excluded.icon,
                daily_goal = excluded.daily_goal,
                current_count = excluded.current_count,
                last_date = excluded.last_date,
                updated_at = excluded.updated_at,
                version = habits.version + 1,
                dirty = 1",
            params![
                id,
                name,
                icon,
                daily_goal.max(1),
                current_count,
                last_date.to_string(),
                now
            ],
        )?;
        Ok(())
    }

    pub fn log_habit_entry(
        &mut self,
        habit_id: &str,
        entry_date: NaiveDate,
        delta: i64,
        new_count: u32,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE habits
             SET current_count = ?2, last_date = ?3, updated_at = ?4, version = version + 1, dirty = 1
             WHERE id = ?1",
            params![habit_id, new_count, entry_date.to_string(), now],
        )?;
        tx.execute(
            "INSERT INTO habit_entries (id, habit_id, entry_date, delta, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, 1)",
            params![
                Uuid::new_v4().to_string(),
                habit_id,
                entry_date.to_string(),
                delta,
                now
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_pomodoro(&self) -> anyhow::Result<Option<PersistedPomodoro>> {
        self.conn
            .query_row(
                "SELECT phase, work_minutes, break_minutes, remaining_secs, completed_sessions
                 FROM pomodoro_state WHERE id = 1",
                [],
                |row| {
                    Ok(PersistedPomodoro {
                        phase: row.get(0)?,
                        work_minutes: row.get(1)?,
                        break_minutes: row.get(2)?,
                        remaining_secs: row.get(3)?,
                        completed_sessions: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_pomodoro(
        &self,
        phase: &str,
        work_minutes: u32,
        break_minutes: u32,
        remaining_secs: u32,
        completed_sessions: u32,
    ) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO pomodoro_state (id, phase, work_minutes, break_minutes, remaining_secs, completed_sessions, updated_at, version, dirty)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 1, 1)
             ON CONFLICT(id) DO UPDATE SET
                phase = excluded.phase,
                work_minutes = excluded.work_minutes,
                break_minutes = excluded.break_minutes,
                remaining_secs = excluded.remaining_secs,
                completed_sessions = excluded.completed_sessions,
                updated_at = excluded.updated_at,
                version = pomodoro_state.version + 1,
                dirty = 1",
            params![
                phase,
                work_minutes,
                break_minutes,
                remaining_secs,
                completed_sessions,
                now
            ],
        )?;
        Ok(())
    }

    pub fn count_conversations(&self) -> anyhow::Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn list_conversations_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<ConversationSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.title, c.created_at, c.updated_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id AND m.deleted_at IS NULL) AS msg_count
             FROM conversations c
             WHERE c.deleted_at IS NULL
             ORDER BY c.updated_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn ensure_conversation(&self, id: &str, title: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO conversations (id, title, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?3, 1, 1)",
            params![id, title, now],
        )?;
        Ok(())
    }

    pub fn update_conversation_title(&self, id: &str, title: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE conversations SET title = ?2, updated_at = ?3, version = version + 1, dirty = 1 WHERE id = ?1",
            params![id, title, now],
        )?;
        Ok(())
    }

    pub fn soft_delete_conversation(&self, id: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE conversations SET deleted_at = ?2, updated_at = ?2, version = version + 1, dirty = 1 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn list_messages(&self, conversation_id: &str) -> anyhow::Result<Vec<PersistedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, seq, role, kind, content, tool_calls_json, tool_call_id, raw_gemini_parts_json, created_at
             FROM messages
             WHERE conversation_id = ?1 AND deleted_at IS NULL
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map([conversation_id], |row| {
            let kind_str: String = row.get(4)?;
            Ok(PersistedMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                seq: row.get(2)?,
                role: row.get(3)?,
                kind: MessageKind::from_str(&kind_str),
                content: row.get(5)?,
                tool_calls_json: row.get(6)?,
                tool_call_id: row.get(7)?,
                raw_gemini_parts_json: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn append_message(&mut self, msg: &NewPersistedMessage) -> anyhow::Result<i64> {
        let tx = self.conn.transaction()?;
        let next_seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq) + 1, 0) FROM messages WHERE conversation_id = ?1",
            [&msg.conversation_id],
            |row| row.get(0),
        )?;

        tx.execute(
            "INSERT INTO messages (id, conversation_id, seq, role, kind, content, tool_calls_json, tool_call_id, raw_gemini_parts_json, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, 1, 1)",
            params![
                msg.id,
                msg.conversation_id,
                next_seq,
                msg.role,
                msg.kind.as_str(),
                msg.content,
                msg.tool_calls_json,
                msg.tool_call_id,
                msg.raw_gemini_parts_json,
                msg.created_at
            ],
        )?;

        tx.execute(
            "UPDATE conversations SET updated_at = ?2, version = version + 1, dirty = 1 WHERE id = ?1",
            params![msg.conversation_id, msg.created_at],
        )?;

        tx.commit()?;
        Ok(next_seq)
    }

    pub fn has_tool_execution(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
    ) -> anyhow::Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tool_executions WHERE conversation_id = ?1 AND tool_call_id = ?2)",
                params![conversation_id, tool_call_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn record_tool_execution(&self, exec: &NewToolExecution) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO tool_executions (id, conversation_id, message_id, tool_call_id, tool_name, source, arguments_json, status, result_summary, result_full, error_message, created_at, updated_at, version, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, 1, 1)",
            params![
                exec.id,
                exec.conversation_id,
                exec.message_id,
                exec.tool_call_id,
                exec.tool_name,
                exec.source,
                exec.arguments_json,
                exec.status,
                exec.result_summary,
                exec.result_full,
                exec.error_message,
                now
            ],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_tool_executions(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<Vec<PersistedToolExecution>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, tool_call_id, tool_name, source, arguments_json, status, result_summary, error_message, created_at
             FROM tool_executions
             WHERE conversation_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([conversation_id], |row| {
            Ok(PersistedToolExecution {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                tool_call_id: row.get(2)?,
                tool_name: row.get(3)?,
                source: row.get(4)?,
                arguments_json: row.get(5)?,
                status: row.get(6)?,
                result_summary: row.get(7)?,
                error_message: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rusty_desk_persist_{label}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sqlite_crud_transactions_fk_and_pagination() {
        let mut db = Database::open_in_memory().unwrap();

        // Referential integrity: inserting message for non-existent conversation must fail
        let bad_msg = NewPersistedMessage {
            id: Uuid::new_v4().to_string(),
            conversation_id: "nonexistent".to_string(),
            role: "user".to_string(),
            kind: MessageKind::Text,
            content: Some("hola".to_string()),
            tool_calls_json: None,
            tool_call_id: None,
            raw_gemini_parts_json: None,
            created_at: Utc::now().to_rfc3339(),
        };
        assert!(db.append_message(&bad_msg).is_err());

        // Insert 65 conversations -> verify no 50-conversation limit + pagination works
        for i in 0..65 {
            let cid = format!("conv-{i:03}");
            db.ensure_conversation(&cid, &format!("Chat {i}")).unwrap();
            db.append_message(&NewPersistedMessage {
                id: Uuid::new_v4().to_string(),
                conversation_id: cid,
                role: "user".to_string(),
                kind: MessageKind::Text,
                content: Some(format!("Mensaje {i}")),
                tool_calls_json: None,
                tool_call_id: None,
                raw_gemini_parts_json: None,
                created_at: format!("2026-11-15T10:{:02}:00Z", i % 60),
            })
            .unwrap();
        }
        assert_eq!(db.count_conversations().unwrap(), 65);
        let page1 = db.list_conversations_page(20, 0).unwrap();
        let page4 = db.list_conversations_page(20, 60).unwrap();
        assert_eq!(page1.len(), 20);
        assert_eq!(page4.len(), 5);

        // Soft delete
        db.soft_delete_conversation("conv-000").unwrap();
        assert_eq!(db.count_conversations().unwrap(), 64);

        // Tasks & habits transactional entry
        db.upsert_task("t1", "Comprar café", false, &Utc::now().to_rfc3339())
            .unwrap();
        assert_eq!(db.list_tasks().unwrap().len(), 1);
        db.soft_delete_task("t1").unwrap();
        assert!(db.list_tasks().unwrap().is_empty());

        let today = chrono::Local::now().date_naive();
        db.upsert_habit("h1", "Agua", "💧", 8, 0, today).unwrap();
        db.log_habit_entry("h1", today, 1, 1).unwrap();
        assert_eq!(db.list_habits().unwrap()[0].current_count, 1);
    }

    #[test]
    fn json_migration_valid_idempotent_and_more_than_50_sessions() {
        let dir = temp_dir("valid_mig");
        let db_path = dir.join("rusty_desk.db");
        let json_path = dir.join("data.json");

        let sessions: Vec<_> = (0..58)
            .map(|i| {
                json!({
                    "id": format!("sess-{i}"),
                    "title": format!("Charla {i}"),
                    "updated_at": "15/11 09:00",
                    "ui_entries": [
                        { "is_user": true, "text": format!("Hola {i}"), "timestamp": "09:00" },
                        { "is_user": false, "text": format!("Respuesta {i}"), "timestamp": "09:01" }
                    ]
                })
            })
            .collect();

        let payload = json!({
            "tasks": [{ "id": "task-1", "title": "Revisar PR", "done": false, "created_at": "2026-11-15 09:00" }],
            "habits": [{ "id": "hab-1", "name": "Agua", "icon": "💧", "daily_goal": 8, "current_count": 3, "last_date": "2026-11-15" }],
            "pomodoro": { "phase": "Work", "work_minutes": 30, "break_minutes": 5, "remaining_secs": 1800, "completed_sessions": 4 },
            "chat_sessions": sessions
        });
        fs::write(&json_path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();

        let (db, outcome) = Database::open_with_json_migration(&db_path, &json_path).unwrap();
        assert_eq!(
            outcome,
            MigrationOutcome::Migrated {
                tasks: 1,
                habits: 1,
                conversations: 58,
                messages: 116
            }
        );
        // Original data.json and .bak must both exist
        assert!(json_path.exists());
        assert!(dir.join("data.json.bak").exists());
        // All 58 conversations preserved with IDs intact
        assert_eq!(db.count_conversations().unwrap(), 58);
        assert_eq!(db.list_messages("sess-42").unwrap().len(), 2);
        assert_eq!(db.load_pomodoro().unwrap().unwrap().completed_sessions, 4);

        // Re-running migration must be idempotent and never duplicate records
        drop(db);
        let (db2, outcome2) = Database::open_with_json_migration(&db_path, &json_path).unwrap();
        assert_eq!(outcome2, MigrationOutcome::AlreadyCompleted);
        assert_eq!(db2.count_conversations().unwrap(), 58);
        assert_eq!(db2.list_messages("sess-42").unwrap().len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_migration_handles_empty_corrupt_and_partial_files() {
        let dir = temp_dir("edge_mig");

        // 1. Empty file
        let empty_json = dir.join("empty.json");
        let empty_db = dir.join("empty.db");
        fs::write(&empty_json, "   ").unwrap();
        let (_db, out) = Database::open_with_json_migration(&empty_db, &empty_json).unwrap();
        assert_eq!(out, MigrationOutcome::EmptyFile);

        // 2. Corrupt file -> must fail explicitly, keep original JSON, and not mark completed
        let corrupt_json = dir.join("corrupt.json");
        let corrupt_db = dir.join("corrupt.db");
        fs::write(&corrupt_json, "{ not valid json").unwrap();
        assert!(Database::open_with_json_migration(&corrupt_db, &corrupt_json).is_err());
        assert!(corrupt_json.exists());

        // 3. Partial/incomplete fields -> fills defaults safely
        let partial_json = dir.join("partial.json");
        let partial_db = dir.join("partial.db");
        fs::write(&partial_json, r#"{"tasks":[{"title":"Solo título"}]}"#).unwrap();
        let (db_partial, out_partial) =
            Database::open_with_json_migration(&partial_db, &partial_json).unwrap();
        assert!(matches!(
            out_partial,
            MigrationOutcome::Migrated { tasks: 1, .. }
        ));
        assert_eq!(db_partial.list_tasks().unwrap()[0].title, "Solo título");

        let _ = fs::remove_dir_all(&dir);
    }
}
