#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

//! Memory crate: types, scanning, selection, extraction, prefetch, and fork
//! guard — the single canonical home for all memory-related logic in novel-agent.

mod frontmatter;
mod guard;
mod loading;
mod memory_extract;
mod memory_extract_prompt;
mod memory_scan;
mod memory_select;
mod memory_types;
mod prefetch;
mod selection;

pub use guard::{
    has_memory_writes_since, is_memory_rel_path, is_memory_write_tool, memory_fork_can_use_tool,
};
pub use loading::load_memory;
pub use memory_extract::{ExtractionContext, MemoryExtractor, PreparedMemoryExtraction};
// build_memory_extraction_prompt, format_memory_manifest, scan_memory_files_for_extraction:
// crate-internal — only used within memory_extract and selection pipelines.
pub use memory_scan::scan_memory_files;
pub use memory_types::{
    estimated_word_count, memory_header, MemoryConstants, MemoryFrontmatter, MemoryHeader,
    MemoryStatus, MemoryType, SideQueryResult, SurfacedMemory,
};
pub use prefetch::MemoryPrefetch;
pub use selection::{create_selector_from_config, MemorySelector};
