use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
    ConnectOptions,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tracing::log::LevelFilter;

use super::schema::{SCHEMA_SQL, SessionSummary, SessionRecord, ChatMessageRecord};

#[derive(Clone)]
pub struct Database {
    pool: Arc<SqlitePool>,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection pool
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure the parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db_url = format!("sqlite://{}", db_path.display());
        
        // Configure connection options
        let connect_options = SqliteConnectOptions::from_str(&db_url)?
            .create_if_missing(true)
            .log_statements(LevelFilter::Debug);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connect_options)
            .await?;

        let db = Self {
            pool: Arc::new(pool),
            path: db_path.to_path_buf(),
        };

        // Initialize schema
        db.initialize_schema().await?;

        Ok(db)
    }

    /// Get the default database path (~/.assistant/assistant.db)
    pub fn default_path() -> Result<PathBuf> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home_dir.join(".assistant").join("assistant.db"))
    }

    /// Initialize the database schema
    async fn initialize_schema(&self) -> Result<()> {
        sqlx::query(SCHEMA_SQL)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get the database path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Execute migrations (placeholder for future use)
    pub async fn migrate(&self) -> Result<()> {
        // TODO: Implement migrations when schema changes
        Ok(())
    }

    /// Test database connection
    pub async fn test_connection(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .fetch_one(&*self.pool)
            .await?;
        Ok(())
    }

    /// List all sessions with their metadata
    pub async fn list_sessions(&self, limit: i64, offset: i64) -> Result<Vec<SessionSummary>> {
        let rows = sqlx::query_as::<_, SessionSummary>(
            r#"
            SELECT 
                s.id,
                s.name,
                s.summary,
                s.created_at,
                s.last_accessed,
                s.updated_at,
                COUNT(DISTINCT m.id) as message_count,
                MAX(m.created_at) as last_message_at
            FROM sessions s
            LEFT JOIN chat_messages m ON s.id = m.session_id
            GROUP BY s.id
            ORDER BY s.last_accessed DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&*self.pool)
        .await?;

        Ok(rows)
    }

    /// Search sessions by name or content
    pub async fn search_sessions(&self, query: &str, limit: i64) -> Result<Vec<SessionSummary>> {
        let search_pattern = format!("%{}%", query);
        
        let rows = sqlx::query_as::<_, SessionSummary>(
            r#"
            SELECT DISTINCT
                s.id,
                s.name,
                s.summary,
                s.created_at,
                s.last_accessed,
                s.updated_at,
                COUNT(DISTINCT m.id) as message_count,
                MAX(m.created_at) as last_message_at
            FROM sessions s
            LEFT JOIN chat_messages m ON s.id = m.session_id
            WHERE s.name LIKE ?1 
               OR s.summary LIKE ?1
               OR EXISTS (
                   SELECT 1 FROM chat_messages cm 
                   WHERE cm.session_id = s.id 
                   AND cm.content LIKE ?1
               )
            GROUP BY s.id
            ORDER BY s.last_accessed DESC
            LIMIT ?2
            "#,
        )
        .bind(&search_pattern)
        .bind(limit)
        .fetch_all(&*self.pool)
        .await?;

        Ok(rows)
    }

    /// Rename a session
    pub async fn rename_session(&self, session_id: &str, new_name: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sessions
            SET name = ?1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?2
            "#,
        )
        .bind(new_name)
        .bind(session_id)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Delete a session and all its messages
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        // Start a transaction
        let mut tx = self.pool.begin().await?;

        // Delete messages first (due to foreign key constraint)
        sqlx::query("DELETE FROM chat_messages WHERE session_id = ?1")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        // Delete todos
        sqlx::query("DELETE FROM todos WHERE session_id = ?1")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        // Delete the session
        sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        // Commit the transaction
        tx.commit().await?;

        Ok(())
    }

    /// Get messages for a session
    pub async fn get_session_messages(
        &self, 
        session_id: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ChatMessageRecord>> {
        let query = if let (Some(limit), Some(offset)) = (limit, offset) {
            sqlx::query_as::<_, ChatMessageRecord>(
                r#"
                SELECT id, session_id, role, content, tool_calls, embedding, created_at
                FROM chat_messages
                WHERE session_id = ?1
                ORDER BY created_at DESC
                LIMIT ?2 OFFSET ?3
                "#,
            )
            .bind(session_id)
            .bind(limit)
            .bind(offset)
        } else {
            sqlx::query_as::<_, ChatMessageRecord>(
                r#"
                SELECT id, session_id, role, content, tool_calls, embedding, created_at
                FROM chat_messages
                WHERE session_id = ?1
                ORDER BY created_at ASC
                "#,
            )
            .bind(session_id)
        };

        let rows = query.fetch_all(&*self.pool).await?;
        Ok(rows)
    }

    /// Get a single session by ID
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        let row = sqlx::query_as::<_, SessionRecord>(
            r#"
            SELECT id, workspace_path, name, summary, summary_embedding, 
                   created_at, last_accessed, updated_at, metadata
            FROM sessions
            WHERE id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&*self.pool)
        .await?;

        Ok(row)
    }

    /// Create a new session
    pub async fn create_session(&self, workspace_path: Option<&str>) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();

        sqlx::query(
            r#"
            INSERT INTO sessions (id, workspace_path, created_at, last_accessed, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(&session_id)
        .bind(workspace_path)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&*self.pool)
        .await?;

        Ok(session_id)
    }

    /// Update session last accessed time
    pub async fn touch_session(&self, session_id: &str) -> Result<()> {
        let now = chrono::Utc::now();
        
        sqlx::query(
            r#"
            UPDATE sessions
            SET last_accessed = ?1, updated_at = ?1
            WHERE id = ?2
            "#,
        )
        .bind(&now)
        .bind(session_id)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }
}