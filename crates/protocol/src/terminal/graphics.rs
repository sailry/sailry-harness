use super::*;

pub const MAX_GRAPHICS_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct Graphics {
    pub images: Vec<Image>,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Image {
    pub id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    /// Base64 PNG; independent of the execution Node's filesystem.
    pub png: std::sync::Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Placement {
    pub image: u32,
    pub id: u32,
    /// Relative to the live viewport; negative rows remain visible in scrollback.
    pub column: i32,
    pub row: i32,
    pub offset: [u32; 2],
    pub size: [u32; 2],
    pub source: [u32; 4],
    /// Unicode placeholder tile: column, row, total columns and total rows.
    pub tile: Option<[u32; 4]>,
    pub cell: [u32; 2],
    pub z: i32,
}
