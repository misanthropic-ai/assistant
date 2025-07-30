use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};

/// Resolve a path (relative or absolute) to a canonical absolute path
pub fn resolve_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    
    // Get absolute path
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;
        cwd.join(path)
    };
    
    // Canonicalize to resolve symlinks and normalize path
    absolute_path.canonicalize()
        .map_err(|e| anyhow!("Cannot access path '{}': {}", path.display(), e))
}

/// Check if a path is within the allowed workspace
/// For now, this is a placeholder - we'll implement proper sandboxing later
pub fn validate_path_access(path: &Path) -> Result<()> {
    // TODO: Implement workspace/sandbox validation
    // For now, allow all paths
    Ok(())
}