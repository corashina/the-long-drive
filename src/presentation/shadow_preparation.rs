//! Inputs owned by each view's retained GPU state. Revisions include all fit inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewRevision {
    pub session: u64,
    pub scene: u32,
    pub fit: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CascadeKey {
    pub view: ViewRevision,
    pub cadence: u64,
    pub bias: [u32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CollectorKey {
    pub view: ViewRevision,
    pub cadence: u64,
    pub projection: [u32; 16],
    pub view_from_world: [u32; 16],
    pub near_far_strength: [u32; 3],
    pub atlas_size: u32,
    pub primary: bool,
}
/// Compare shader values by bits. Signed zero must not hide an input change.
pub(crate) fn same_floats(a: impl Iterator<Item = f32>, b: impl Iterator<Item = f32>) -> bool {
    a.map(f32::to_bits).eq(b.map(f32::to_bits))
}
