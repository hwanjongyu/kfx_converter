//! Shared data structures for packaging pipeline stages.

/// Packaging metadata for an individual section.
#[derive(Debug, Clone)]
pub struct SectionPackData {
    pub raw_id: String,
    pub name: String,
    pub sec_sym_id: u32,
    pub sec_eid: u32,
    pub story_sym_id: u32,
    pub block_info: Vec<(u32, usize)>,
}
