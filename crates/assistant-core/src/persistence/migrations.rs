use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use super::database::Database;
use crate::actors::tools::todo::{Todo, TodoPriority, TodoStatus};

/// Migration manager for handling data migrations
pub struct MigrationManager {
    db: Database,
}

impl MigrationManager {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Migrate existing todos.json to the database
    pub async fn migrate_todos_json(&self, json_path: &Path, session_id: &str) -> Result<()> {
        if !json_path.exists() {
            tracing::info!("No existing todos.json found at {:?}", json_path);
            return Ok(());
        }

        tracing::info!("Migrating todos from {:?}", json_path);

        // Read existing todos
        let content = std::fs::read_to_string(json_path)?;
        let todos: HashMap<String, Todo> = serde_json::from_str(&content)?;
        
        let todo_count = todos.len();

        // Insert each todo into the database
        for (_, todo) in todos {
            let status = match todo.status {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            };

            let priority = match todo.priority {
                TodoPriority::Low => "low",
                TodoPriority::Medium => "medium",
                TodoPriority::High => "high",
            };

            sqlx::query(
                r#"
                INSERT INTO todos (id, session_id, content, status, priority)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(id) DO NOTHING
                "#,
            )
            .bind(&todo.id)
            .bind(session_id)
            .bind(&todo.content)
            .bind(status)
            .bind(priority)
            .execute(self.db.pool())
            .await?;
        }

        // Optionally rename the old file to indicate it's been migrated
        let backup_path = json_path.with_extension("json.migrated");
        std::fs::rename(json_path, backup_path)?;

        tracing::info!("Successfully migrated {} todos", todo_count);
        Ok(())
    }

    /// Check if migrations are needed
    pub async fn check_migrations_needed(&self) -> Result<Vec<String>> {
        let mut needed_migrations = Vec::new();

        // Check for todos.json file
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        let todos_path = home_dir.join(".assistant").join("todos.json");
        
        if todos_path.exists() {
            needed_migrations.push("todos.json".to_string());
        }

        Ok(needed_migrations)
    }

    /// Run all pending migrations
    pub async fn run_all_migrations(&self, session_id: &str) -> Result<()> {
        let needed = self.check_migrations_needed().await?;

        for migration in needed {
            match migration.as_str() {
                "todos.json" => {
                    let home_dir = dirs::home_dir()
                        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
                    let todos_path = home_dir.join(".assistant").join("todos.json");
                    self.migrate_todos_json(&todos_path, session_id).await?;
                }
                _ => {
                    tracing::warn!("Unknown migration: {}", migration);
                }
            }
        }

        Ok(())
    }
}