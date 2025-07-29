pub mod base;
pub mod tool_registry;

// Individual tool actors
pub mod read;
pub mod edit;
pub mod write;
pub mod ls;
pub mod glob;
pub mod grep;
pub mod bash;
pub mod web_search;
pub mod web_fetch;
pub mod todo_write;
pub mod memory;
pub mod read_many_files;

// Re-export message type and registry
pub use crate::messages::ToolMessage;
pub use tool_registry::ToolRegistry;

// Re-export all tool actors
pub use read::ReadActor;
pub use edit::EditActor;
pub use write::WriteActor;
pub use ls::LsActor;
pub use glob::GlobActor;
pub use grep::GrepActor;
pub use bash::BashActor;
pub use web_search::WebSearchActor;
pub use web_fetch::WebFetchActor;
pub use todo_write::TodoWriteActor;
pub use memory::MemoryActor;
pub use read_many_files::ReadManyFilesActor;