use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
    ConnectOptions,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tracing::log::LevelFilter;

use super::schema::SCHEMA_SQL;

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
}