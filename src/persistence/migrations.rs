use chrono::Utc;
use rusqlite::Connection;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "001_initial_schema",
        sql: r#"
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_conversations_updated
            ON conversations(deleted_at, updated_at DESC);

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            role TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT,
            tool_calls_json TEXT,
            tool_call_id TEXT,
            raw_gemini_parts_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_messages_conv_seq
            ON messages(conversation_id, seq ASC);

        CREATE TABLE IF NOT EXISTS tool_executions (
            id TEXT PRIMARY KEY,
            conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
            message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
            tool_call_id TEXT,
            tool_name TEXT NOT NULL,
            source TEXT NOT NULL,
            arguments_json TEXT NOT NULL,
            status TEXT NOT NULL,
            result_summary TEXT,
            result_full TEXT,
            error_message TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_tool_exec_conv
            ON tool_executions(conversation_id, created_at DESC);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_exec_unique_call
            ON tool_executions(conversation_id, tool_call_id)
            WHERE conversation_id IS NOT NULL AND tool_call_id IS NOT NULL;

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_tasks_active
            ON tasks(deleted_at, done, created_at ASC);

        CREATE TABLE IF NOT EXISTS habits (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            icon TEXT NOT NULL,
            daily_goal INTEGER NOT NULL,
            current_count INTEGER NOT NULL DEFAULT 0,
            last_date TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_habits_active
            ON habits(deleted_at, created_at ASC);

        CREATE TABLE IF NOT EXISTS habit_entries (
            id TEXT PRIMARY KEY,
            habit_id TEXT NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
            entry_date TEXT NOT NULL,
            delta INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_habit_entries_habit_date
            ON habit_entries(habit_id, entry_date);

        CREATE TABLE IF NOT EXISTS pomodoro_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            phase TEXT NOT NULL,
            work_minutes INTEGER NOT NULL,
            break_minutes INTEGER NOT NULL,
            remaining_secs INTEGER NOT NULL,
            completed_sessions INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            dirty INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
    "#,
    },
    Migration {
        version: 2,
        name: "0002_music_playlists",
        sql: r#"
        CREATE TABLE IF NOT EXISTS music_playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            source_path TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS music_tracks (
            id TEXT PRIMARY KEY,
            playlist_id TEXT NOT NULL REFERENCES music_playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            title TEXT NOT NULL,
            uri TEXT NOT NULL,
            duration_secs INTEGER,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_music_tracks_playlist_pos
            ON music_tracks(playlist_id, position ASC);
    "#,
    },
    Migration {
        version: 3,
        name: "0003_focus_sessions",
        sql: r#"
        ALTER TABLE pomodoro_state ADD COLUMN active_task_id TEXT;
        ALTER TABLE pomodoro_state ADD COLUMN active_task_title TEXT;
        ALTER TABLE pomodoro_state ADD COLUMN running INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE pomodoro_state ADD COLUMN phase_ref_utc TEXT;
        ALTER TABLE pomodoro_state ADD COLUMN phase_ref_remaining_secs INTEGER NOT NULL DEFAULT 1500;
        ALTER TABLE pomodoro_state ADD COLUMN active_session_id TEXT;
        ALTER TABLE pomodoro_state ADD COLUMN session_started_at TEXT;
        ALTER TABLE pomodoro_state ADD COLUMN pending_prompt TEXT;

        CREATE TABLE IF NOT EXISTS focus_sessions (
            id TEXT PRIMARY KEY,
            task_id TEXT,
            task_title TEXT,
            started_at TEXT NOT NULL,
            ended_at TEXT NOT NULL,
            duration_secs INTEGER NOT NULL,
            status TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_focus_sessions_ended_at
            ON focus_sessions(ended_at DESC);
    "#,
    },
];

pub fn run_migrations(conn: &mut Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    for m in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [m.version],
            |row| row.get(0),
        )?;
        if already_applied {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![m.version, m.name, Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
    }

    Ok(())
}
