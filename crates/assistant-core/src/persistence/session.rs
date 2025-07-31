use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::Path;
use uuid::Uuid;

use super::database::Database;
use super::schema::SessionRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// Each run gets a new session
    PerRun,
    /// Session is derived from workspace path
    PerWorkspace,
    /// Global session shared across all runs
    Global,
    /// Explicit session ID provided
    Explicit(String),
}

impl Default for SessionMode {
    fn default() -> Self {
        SessionMode::PerRun
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub workspace_path: Option<String>,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub summary_embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

impl From<SessionRecord> for Session {
    fn from(record: SessionRecord) -> Self {
        Self {
            id: record.id,
            workspace_path: record.workspace_path,
            name: record.name,
            summary: record.summary,
            summary_embedding: record.summary_embedding,
            created_at: record.created_at,
            last_accessed: record.last_accessed,
            updated_at: record.updated_at,
            metadata: record.metadata,
        }
    }
}

pub struct SessionManager {
    db: Database,
}

impl SessionManager {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get or create a session based on the mode
    pub async fn get_or_create_session(
        &self,
        mode: &SessionMode,
        workspace_path: Option<&Path>,
    ) -> Result<Session> {
        match mode {
            SessionMode::PerRun => {
                // Always create a new session
                self.create_session(workspace_path).await
            }
            SessionMode::PerWorkspace => {
                if let Some(workspace) = workspace_path {
                    // Derive session ID from workspace path
                    let session_id = self.derive_session_id_from_path(workspace);
                    self.get_or_create_session_by_id(&session_id, Some(workspace))
                        .await
                } else {
                    // No workspace, create new session
                    self.create_session(None).await
                }
            }
            SessionMode::Global => {
                // Use a fixed global session ID
                self.get_or_create_session_by_id("global", workspace_path)
                    .await
            }
            SessionMode::Explicit(id) => {
                // Use the provided session ID
                self.get_or_create_session_by_id(id, workspace_path).await
            }
        }
    }

    /// Create a new session
    async fn create_session(&self, workspace_path: Option<&Path>) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let workspace_str = workspace_path.map(|p| p.display().to_string());

        sqlx::query(
            r#"
            INSERT INTO sessions (id, workspace_path, created_at, last_accessed, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(&id)
        .bind(&workspace_str)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(self.db.pool())
        .await?;

        Ok(Session {
            id,
            workspace_path: workspace_str,
            name: None,
            summary: None,
            summary_embedding: None,
            created_at: now,
            last_accessed: now,
            updated_at: now,
            metadata: None,
        })
    }

    /// Get or create a session by ID
    async fn get_or_create_session_by_id(
        &self,
        session_id: &str,
        workspace_path: Option<&Path>,
    ) -> Result<Session> {
        // Try to get existing session
        if let Some(session) = self.get_session(session_id).await? {
            // Update last accessed time
            self.update_last_accessed(session_id).await?;
            Ok(session)
        } else {
            // Create new session with specific ID
            let now = Utc::now();
            let workspace_str = workspace_path.map(|p| p.display().to_string());

            sqlx::query(
                r#"
                INSERT INTO sessions (id, workspace_path, created_at, last_accessed, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
            )
            .bind(session_id)
            .bind(&workspace_str)
            .bind(&now)
            .bind(&now)
            .bind(&now)
            .execute(self.db.pool())
            .await?;

            Ok(Session {
                id: session_id.to_string(),
                workspace_path: workspace_str,
                name: None,
                summary: None,
                summary_embedding: None,
                created_at: now,
                last_accessed: now,
                updated_at: now,
                metadata: None,
            })
        }
    }

    /// Get a session by ID
    async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_path, name, summary, summary_embedding, 
                   created_at, last_accessed, updated_at, metadata
            FROM sessions
            WHERE id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(self.db.pool())
        .await?;

        if let Some(row) = row {
            // Deserialize embedding from bytes if present
            let embedding_bytes: Option<Vec<u8>> = row.get(4);
            let summary_embedding = embedding_bytes.and_then(|bytes| {
                if bytes.len() % 4 == 0 {
                    Some(
                        bytes
                            .chunks(4)
                            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                            .collect()
                    )
                } else {
                    None
                }
            });

            Ok(Some(Session {
                id: row.get(0),
                workspace_path: row.get(1),
                name: row.get(2),
                summary: row.get(3),
                summary_embedding,
                created_at: row.get(5),
                last_accessed: row.get(6),
                updated_at: row.get(7),
                metadata: row.get::<Option<String>, _>(8)
                    .and_then(|s| serde_json::from_str(&s).ok()),
            }))
        } else {
            Ok(None)
        }
    }

    /// Update last accessed time for a session
    async fn update_last_accessed(&self, session_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sessions
            SET last_accessed = ?1
            WHERE id = ?2
            "#,
        )
        .bind(Utc::now())
        .bind(session_id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    /// Derive a stable session ID from a workspace path
    fn derive_session_id_from_path(&self, path: &Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let hash = hasher.finish();
        format!("workspace_{:x}", hash)
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query(
            r#"
            SELECT id, workspace_path, name, summary, summary_embedding,
                   created_at, last_accessed, updated_at, metadata
            FROM sessions
            ORDER BY last_accessed DESC
            "#,
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                // Deserialize embedding from bytes if present
                let embedding_bytes: Option<Vec<u8>> = row.get(4);
                let summary_embedding = embedding_bytes.and_then(|bytes| {
                    if bytes.len() % 4 == 0 {
                        Some(
                            bytes
                                .chunks(4)
                                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                                .collect()
                        )
                    } else {
                        None
                    }
                });

                Session {
                    id: row.get(0),
                    workspace_path: row.get(1),
                    name: row.get(2),
                    summary: row.get(3),
                    summary_embedding,
                    created_at: row.get(5),
                    last_accessed: row.get(6),
                    updated_at: row.get(7),
                    metadata: row.get::<Option<String>, _>(8)
                        .and_then(|s| serde_json::from_str(&s).ok()),
                }
            })
            .collect())
    }

    /// Delete old sessions that haven't been accessed in the specified number of days
    pub async fn cleanup_old_sessions(&self, days: i64) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        
        let result = sqlx::query(
            r#"
            DELETE FROM sessions
            WHERE last_accessed < ?1
            "#,
        )
        .bind(cutoff)
        .execute(self.db.pool())
        .await?;

        Ok(result.rows_affected())
    }
}