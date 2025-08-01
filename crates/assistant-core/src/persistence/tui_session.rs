use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use super::database::Database;
use super::schema::TuiSessionRecord;

/// Status values for TUI sessions
pub mod status {
    pub const ACTIVE: &str = "active";
    pub const PAUSED: &str = "paused";
    pub const TERMINATED: &str = "terminated";
}

pub struct TuiSessionManager {
    db: Database,
}

impl TuiSessionManager {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Create a new TUI session
    pub async fn create_session(
        &self,
        chat_session_id: Option<&str>,
        tmux_session_name: &str,
        command: &str,
    ) -> Result<TuiSessionRecord> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let record = TuiSessionRecord {
            id: id.clone(),
            chat_session_id: chat_session_id.map(|s| s.to_string()),
            tmux_session_name: tmux_session_name.to_string(),
            command: command.to_string(),
            status: status::ACTIVE.to_string(),
            created_at: now,
            last_accessed: now,
            metadata: None,
        };

        sqlx::query(
            r#"
            INSERT INTO tui_sessions (
                id, chat_session_id, tmux_session_name, command, 
                status, created_at, last_accessed, metadata
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&record.id)
        .bind(&record.chat_session_id)
        .bind(&record.tmux_session_name)
        .bind(&record.command)
        .bind(&record.status)
        .bind(&record.created_at)
        .bind(&record.last_accessed)
        .bind(record.metadata.as_ref().map(|m| m.to_string()))
        .execute(self.db.pool())
        .await?;

        Ok(record)
    }

    /// Get a TUI session by ID
    pub async fn get_session(&self, id: &str) -> Result<Option<TuiSessionRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, chat_session_id, tmux_session_name, command,
                   status, created_at, last_accessed, metadata
            FROM tui_sessions
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(self.row_to_record(row))
    }

    /// Get a TUI session by tmux session name
    pub async fn get_session_by_tmux_name(&self, name: &str) -> Result<Option<TuiSessionRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, chat_session_id, tmux_session_name, command,
                   status, created_at, last_accessed, metadata
            FROM tui_sessions
            WHERE tmux_session_name = ?1
            "#,
        )
        .bind(name)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(self.row_to_record(row))
    }

    /// List all active TUI sessions
    pub async fn list_active_sessions(&self) -> Result<Vec<TuiSessionRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, chat_session_id, tmux_session_name, command,
                   status, created_at, last_accessed, metadata
            FROM tui_sessions
            WHERE status = ?1
            ORDER BY last_accessed DESC
            "#,
        )
        .bind(status::ACTIVE)
        .fetch_all(self.db.pool())
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| self.row_to_record(Some(row)))
            .collect())
    }

    /// List all TUI sessions (including terminated)
    pub async fn list_all_sessions(&self) -> Result<Vec<TuiSessionRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, chat_session_id, tmux_session_name, command,
                   status, created_at, last_accessed, metadata
            FROM tui_sessions
            ORDER BY last_accessed DESC
            "#,
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| self.row_to_record(Some(row)))
            .collect())
    }

    /// Update the last accessed time for a session
    pub async fn update_last_accessed(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tui_sessions
            SET last_accessed = ?1
            WHERE id = ?2
            "#,
        )
        .bind(Utc::now())
        .bind(id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Update the status of a session
    pub async fn update_status(&self, id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tui_sessions
            SET status = ?1, last_accessed = ?2
            WHERE id = ?3
            "#,
        )
        .bind(status)
        .bind(Utc::now())
        .bind(id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Update session metadata
    pub async fn update_metadata(&self, id: &str, metadata: serde_json::Value) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tui_sessions
            SET metadata = ?1, last_accessed = ?2
            WHERE id = ?3
            "#,
        )
        .bind(metadata.to_string())
        .bind(Utc::now())
        .bind(id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Clean up stale sessions that haven't been accessed in the specified number of hours
    pub async fn cleanup_stale_sessions(&self, hours: i64) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::hours(hours);

        // First, get the sessions to clean up
        let stale_sessions = sqlx::query(
            r#"
            SELECT tmux_session_name
            FROM tui_sessions
            WHERE last_accessed < ?1 AND status != ?2
            "#,
        )
        .bind(cutoff)
        .bind(status::TERMINATED)
        .fetch_all(self.db.pool())
        .await?;

        // Kill tmux sessions if they exist
        for row in &stale_sessions {
            let tmux_name: String = row.get(0);
            if let Err(e) = self.kill_tmux_session(&tmux_name).await {
                tracing::warn!("Failed to kill tmux session {}: {}", tmux_name, e);
            }
        }

        // Update database records to terminated
        let result = sqlx::query(
            r#"
            UPDATE tui_sessions
            SET status = ?1
            WHERE last_accessed < ?2 AND status != ?3
            "#,
        )
        .bind(status::TERMINATED)
        .bind(cutoff)
        .bind(status::TERMINATED)
        .execute(self.db.pool())
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete terminated sessions older than the specified days
    pub async fn delete_old_terminated_sessions(&self, days: i64) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::days(days);

        let result = sqlx::query(
            r#"
            DELETE FROM tui_sessions
            WHERE status = ?1 AND last_accessed < ?2
            "#,
        )
        .bind(status::TERMINATED)
        .bind(cutoff)
        .execute(self.db.pool())
        .await?;

        Ok(result.rows_affected())
    }

    /// Verify that active sessions in the database match actual tmux sessions
    pub async fn verify_sessions(&self) -> Result<()> {
        let active_sessions = self.list_active_sessions().await?;

        for session in active_sessions {
            if !self.tmux_session_exists(&session.tmux_session_name).await? {
                tracing::info!(
                    "TUI session {} no longer exists in tmux, marking as terminated",
                    session.id
                );
                self.update_status(&session.id, status::TERMINATED).await?;
            }
        }

        Ok(())
    }

    /// Helper to convert a database row to a TuiSessionRecord
    fn row_to_record(&self, row: Option<sqlx::sqlite::SqliteRow>) -> Option<TuiSessionRecord> {
        row.map(|r| TuiSessionRecord {
            id: r.get(0),
            chat_session_id: r.get(1),
            tmux_session_name: r.get(2),
            command: r.get(3),
            status: r.get(4),
            created_at: r.get(5),
            last_accessed: r.get(6),
            metadata: r
                .get::<Option<String>, _>(7)
                .and_then(|s| serde_json::from_str(&s).ok()),
        })
    }

    /// Check if a tmux session exists
    async fn tmux_session_exists(&self, session_name: &str) -> Result<bool> {
        use tokio::process::Command;

        let output = Command::new("tmux")
            .arg("has-session")
            .arg("-t")
            .arg(session_name)
            .output()
            .await?;

        Ok(output.status.success())
    }

    /// Kill a tmux session
    async fn kill_tmux_session(&self, session_name: &str) -> Result<()> {
        use tokio::process::Command;

        Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(session_name)
            .output()
            .await?;

        Ok(())
    }
}