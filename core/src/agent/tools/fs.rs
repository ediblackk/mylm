//! DEPRECATED: Filesystem tools - read and write files
//!
//! This module is deprecated. Use the following modules directly:
//! - `read_file` - ReadFileTool with chunking and search support
//! - `write_file` - WriteFileTool
//!
//! These modules will be removed in a future version.

#![deprecated(
    since = "0.2.0",
    note = "Use read_file and write_file modules directly. This module will be removed in a future version."
)]

// Re-export from new modules for backward compatibility
pub use super::read_file::ReadFileTool;
pub use super::write_file::WriteFileTool;
