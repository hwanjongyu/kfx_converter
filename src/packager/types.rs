//! Shared data structures for packaging pipeline stages.

/// Pre-allocated collision-free local symbol assignment for a raster image resource (`$164` and `$417`).
#[derive(Debug, Clone)]
pub(crate) struct AllocatedResourceSym {
    pub res_sym_id: u32,
    pub res_sym_name: String,
    pub raw_sym_id: u32,
    pub raw_path: String,
}

/// Packaging metadata for an individual section.
#[derive(Debug, Clone)]
pub struct SectionPackData {
    pub raw_id: String,
    pub name: String,
    pub sec_sym_id: u32,
    pub sec_eid: u32,
    pub story_sym_id: u32,
    pub block_info: Vec<(u32, usize)>,
    pub outbound_anchors: Vec<u32>,
    pub used_resources: Vec<u32>,
}
