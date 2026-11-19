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
    pub active_task_id: Option<String>,
    pub active_task_title: Option<String>,
    pub running: bool,
    pub phase_ref_utc: Option<String>,
    pub phase_ref_remaining_secs: u32,
    pub active_session_id: Option<String>,
    pub session_started_at: Option<String>,
    pub pending_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSessionRecord {
    pub id: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub duration_secs: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedPlaylist {
    pub id: String,
    pub name: String,
    pub source_path: Option<String>,
    pub track_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedTrack {
    pub id: String,
    pub playlist_id: String,
    pub position: usize,
    pub title: String,
    pub uri: String,
    pub duration_secs: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewMusicTrack {
    pub title: String,
    pub uri: String,
    pub duration_secs: Option<i64>,
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
    /// Only conversational text is sent to Dreaming (no tool payloads or hidden system prompts).
    pub fn dreaming_pending_messages(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.rowid, m.role, m.content, m.id FROM messages m
             JOIN conversations c ON c.id = m.conversation_id
             WHERE m.rowid > (SELECT last_message_rowid FROM dreaming_state WHERE id = 1)
               AND m.deleted_at IS NULL AND c.deleted_at IS NULL
               AND m.kind = 'text' AND m.role IN ('user','assistant')
               AND m.content IS NOT NULL AND LENGTH(TRIM(m.content)) > 0
             ORDER BY m.rowid ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn dreaming_store_batch(
        &mut self,
        last_rowid: i64,
        source_id: &str,
        facts: &[String],
    ) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        for content in facts {
            tx.execute("INSERT OR IGNORE INTO dreaming_memories (id, content, source_message_id, created_at)
                        VALUES (?1, ?2, ?3, ?4)", params![Uuid::new_v4().to_string(), content, source_id, Utc::now().to_rfc3339()])?;
        }
        tx.execute(
            "UPDATE dreaming_state SET last_message_rowid = ?1 WHERE id = 1",
            [last_rowid],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn dreaming_list_memories(&self) -> anyhow::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content FROM dreaming_memories ORDER BY created_at DESC LIMIT 200",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn dreaming_delete_memory(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM dreaming_memories WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Small, deterministic keyword retrieval; semantic embeddings can be added later.
    pub fn dreaming_relevant_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<String>> {
        let terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.chars().count() >= 4)
            .map(|s| s.to_lowercase())
            .take(12)
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let memories = self.dreaming_list_memories()?;
        let mut scored: Vec<(usize, String)> = memories
            .into_iter()
            .filter_map(|(_, content)| {
                let lower = content.to_lowercase();
                let score = terms
                    .iter()
                    .filter(|term| lower.contains(term.as_str()))
                    .count();
                (score > 0).then_some((score, content))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, content)| content)
            .collect())
    }

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
                "SELECT phase, work_minutes, break_minutes, remaining_secs, completed_sessions,
                        active_task_id, active_task_title, running, phase_ref_utc,
                        phase_ref_remaining_secs, active_session_id, session_started_at, pending_prompt
                 FROM pomodoro_state WHERE id = 1",
                [],
                |row| {
                    Ok(PersistedPomodoro {
                        phase: row.get(0)?,
                        work_minutes: row.get(1)?,
                        break_minutes: row.get(2)?,
                        remaining_secs: row.get(3)?,
                        completed_sessions: row.get(4)?,
                        active_task_id: row.get(5)?,
                        active_task_title: row.get(6)?,
                        running: row.get::<_, i64>(7)? != 0,
                        phase_ref_utc: row.get(8)?,
                        phase_ref_remaining_secs: row.get(9)?,
                        active_session_id: row.get(10)?,
                        session_started_at: row.get(11)?,
                        pending_prompt: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_pomodoro(&self, p: &PersistedPomodoro) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO pomodoro_state (
                id, phase, work_minutes, break_minutes, remaining_secs, completed_sessions,
                active_task_id, active_task_title, running, phase_ref_utc,
                phase_ref_remaining_secs, active_session_id, session_started_at, pending_prompt,
                updated_at, version, dirty
             )
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1, 1)
             ON CONFLICT(id) DO UPDATE SET
                phase = excluded.phase,
                work_minutes = excluded.work_minutes,
                break_minutes = excluded.break_minutes,
                remaining_secs = excluded.remaining_secs,
                completed_sessions = excluded.completed_sessions,
                active_task_id = excluded.active_task_id,
                active_task_title = excluded.active_task_title,
                running = excluded.running,
                phase_ref_utc = excluded.phase_ref_utc,
                phase_ref_remaining_secs = excluded.phase_ref_remaining_secs,
                active_session_id = excluded.active_session_id,
                session_started_at = excluded.session_started_at,
                pending_prompt = excluded.pending_prompt,
                updated_at = excluded.updated_at,
                version = pomodoro_state.version + 1,
                dirty = 1",
            params![
                p.phase,
                p.work_minutes,
                p.break_minutes,
                p.remaining_secs,
                p.completed_sessions,
                p.active_task_id,
                p.active_task_title,
                if p.running { 1 } else { 0 },
                p.phase_ref_utc,
                p.phase_ref_remaining_secs,
                p.active_session_id,
                p.session_started_at,
                p.pending_prompt,
                now
            ],
        )?;
        Ok(())
    }

    /// Records a completed or interrupted focus session idempotently (`INSERT OR IGNORE`).
    pub fn record_focus_session(&self, s: &FocusSessionRecord) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO focus_sessions (id, task_id, task_title, started_at, ended_at, duration_secs, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                s.id,
                s.task_id,
                s.task_title,
                s.started_at,
                s.ended_at,
                s.duration_secs,
                s.status,
            ],
        )?;
        Ok(())
    }

    /// Lists all recorded focus sessions ordered newest first.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_focus_sessions(&self) -> anyhow::Result<Vec<FocusSessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, task_title, started_at, ended_at, duration_secs, status
             FROM focus_sessions
             ORDER BY ended_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FocusSessionRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_title: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                duration_secs: row.get(5)?,
                status: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns `(completed_count, total_focus_secs)` for sessions ended within `[start_utc, end_utc)`.
    pub fn focus_summary_in_range(
        &self,
        start_utc: &str,
        end_utc: &str,
    ) -> anyhow::Result<(u32, u64)> {
        self.conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(duration_secs), 0)
                 FROM focus_sessions
                 WHERE status = 'Completed' AND ended_at >= ?1 AND ended_at < ?2",
                params![start_utc, end_utc],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u64>(1)?)),
            )
            .map_err(Into::into)
    }

    /// Soft-deletes all completed tasks without touching historical `focus_sessions`.
    pub fn delete_completed_tasks(&self) -> anyhow::Result<usize> {
        let now = Utc::now().to_rfc3339();
        let count = self.conn.execute(
            "UPDATE tasks SET deleted_at = ?1, updated_at = ?1, version = version + 1, dirty = 1
             WHERE deleted_at IS NULL AND done = 1",
            params![now],
        )?;
        Ok(count)
    }

    /// Atomically reserves `estimated_credits` for `(provider_id, period_key)` if within `budget`.
    /// Returns `Ok(true)` if reserved, `Ok(false)` if quota would be exceeded.
    pub fn reserve_external_usage(
        &mut self,
        id: &str,
        tool_name: &str,
        provider_id: &str,
        period_key: &str,
        estimated_credits: u32,
        budget: u32,
    ) -> anyhow::Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let used: u32 = tx.query_row(
            "SELECT COALESCE(SUM(COALESCE(confirmed_credits, estimated_credits)), 0)
             FROM external_tool_usage
             WHERE provider_id = ?1 AND period_key = ?2 AND status IN ('Reserved', 'Success')",
            params![provider_id, period_key],
            |row| row.get(0),
        )?;
        if used.saturating_add(estimated_credits) > budget {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO external_tool_usage (id, tool_name, provider_id, period_key, estimated_credits, confirmed_credits, status, error_kind, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'Reserved', NULL, ?6)",
            params![
                id,
                tool_name,
                provider_id,
                period_key,
                estimated_credits,
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Finalizes a previously reserved external tool usage record (`Success` or `Failed`).
    pub fn finalize_external_usage(
        &self,
        id: &str,
        status: &str,
        confirmed_credits: Option<u32>,
        error_kind: Option<&str>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE external_tool_usage
             SET status = ?2, confirmed_credits = COALESCE(?3, confirmed_credits), error_kind = ?4
             WHERE id = ?1",
            params![id, status, confirmed_credits, error_kind],
        )?;
        Ok(())
    }

    /// Returns `(estimated_or_confirmed_used, has_confirmed)` for `(provider_id, period_key)`.
    pub fn sum_external_usage_for_period(
        &self,
        provider_id: &str,
        period_key: &str,
    ) -> anyhow::Result<(u32, bool)> {
        self.conn
            .query_row(
                "SELECT
                    COALESCE(SUM(COALESCE(confirmed_credits, estimated_credits)), 0),
                    COALESCE(MAX(CASE WHEN confirmed_credits IS NOT NULL THEN 1 ELSE 0 END), 0)
                 FROM external_tool_usage
                 WHERE provider_id = ?1 AND period_key = ?2 AND status IN ('Reserved', 'Success')",
                params![provider_id, period_key],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .map_err(Into::into)
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

    pub fn save_playlist_with_tracks(
        &mut self,
        name: &str,
        source_path: Option<&str>,
        tracks: &[NewMusicTrack],
    ) -> anyhow::Result<PersistedPlaylist> {
        let now = Utc::now().to_rfc3339();
        let playlist_id = Uuid::new_v4().to_string();

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO music_playlists (id, name, source_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![playlist_id, name, source_path, now],
        )?;

        for (pos, track) in tracks.iter().enumerate() {
            let track_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO music_tracks (id, playlist_id, position, title, uri, duration_secs, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    track_id,
                    playlist_id,
                    pos as i64,
                    track.title,
                    track.uri,
                    track.duration_secs,
                    now
                ],
            )?;
        }
        tx.commit()?;

        Ok(PersistedPlaylist {
            id: playlist_id,
            name: name.to_string(),
            source_path: source_path.map(str::to_string),
            track_count: tracks.len(),
            created_at: now,
        })
    }

    pub fn list_playlists(&self) -> anyhow::Result<Vec<PersistedPlaylist>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.source_path, p.created_at, COUNT(t.id)
             FROM music_playlists p
             LEFT JOIN music_tracks t ON t.playlist_id = p.id
             GROUP BY p.id
             ORDER BY p.created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let count: i64 = row.get(4)?;
            Ok(PersistedPlaylist {
                id: row.get(0)?,
                name: row.get(1)?,
                source_path: row.get(2)?,
                created_at: row.get(3)?,
                track_count: count.max(0) as usize,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_tracks(&self, playlist_id: &str) -> anyhow::Result<Vec<PersistedTrack>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, playlist_id, position, title, uri, duration_secs
             FROM music_tracks
             WHERE playlist_id = ?1
             ORDER BY position ASC",
        )?;
        let rows = stmt.query_map([playlist_id], |row| {
            let pos: i64 = row.get(2)?;
            Ok(PersistedTrack {
                id: row.get(0)?,
                playlist_id: row.get(1)?,
                position: pos.max(0) as usize,
                title: row.get(3)?,
                uri: row.get(4)?,
                duration_secs: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn add_track_to_playlist(
        &mut self,
        playlist_id: &str,
        title: &str,
        uri: &str,
        duration_secs: Option<i64>,
    ) -> anyhow::Result<PersistedTrack> {
        let now = Utc::now().to_rfc3339();
        let track_id = Uuid::new_v4().to_string();

        let tx = self.conn.transaction()?;
        let next_pos: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM music_tracks WHERE playlist_id = ?1",
            [playlist_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO music_tracks (id, playlist_id, position, title, uri, duration_secs, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![track_id, playlist_id, next_pos, title, uri, duration_secs, now],
        )?;
        tx.execute(
            "UPDATE music_playlists SET updated_at = ?2 WHERE id = ?1",
            params![playlist_id, now],
        )?;
        tx.commit()?;

        Ok(PersistedTrack {
            id: track_id,
            playlist_id: playlist_id.to_string(),
            position: next_pos.max(0) as usize,
            title: title.to_string(),
            uri: uri.to_string(),
            duration_secs,
        })
    }

    pub fn delete_track(&self, track_id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM music_tracks WHERE id = ?1", [track_id])?;
        Ok(())
    }

    pub fn delete_playlist(&self, playlist_id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM music_playlists WHERE id = ?1", [playlist_id])?;
        Ok(())
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

    #[test]
    fn sqlite_music_playlists_crud_and_cascade_delete() {
        let dir = temp_dir("music_db");
        let db_path = dir.join("rusty.db");
        let mut db = Database::open(&db_path).unwrap();

        let pl = db
            .save_playlist_with_tracks(
                "LoFi Study",
                Some("/tmp/lofi.m3u"),
                &[
                    NewMusicTrack {
                        title: "Track A".to_string(),
                        uri: "/tmp/a.mp3".to_string(),
                        duration_secs: Some(180),
                    },
                    NewMusicTrack {
                        title: "Track B".to_string(),
                        uri: "https://example.com/stream.mp3".to_string(),
                        duration_secs: None,
                    },
                ],
            )
            .unwrap();

        let playlists = db.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "LoFi Study");
        assert_eq!(playlists[0].track_count, 2);

        let tracks = db.list_tracks(&pl.id).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Track A");
        assert_eq!(tracks[1].title, "Track B");

        let added = db
            .add_track_to_playlist(&pl.id, "Radio C", "https://example.com/live", None)
            .unwrap();
        assert_eq!(added.position, 2);
        assert_eq!(db.list_tracks(&pl.id).unwrap().len(), 3);

        db.delete_track(&added.id).unwrap();
        assert_eq!(db.list_tracks(&pl.id).unwrap().len(), 2);

        db.delete_playlist(&pl.id).unwrap();
        assert!(db.list_playlists().unwrap().is_empty());
        assert!(db.list_tracks(&pl.id).unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
