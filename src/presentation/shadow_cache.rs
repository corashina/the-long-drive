/// Exact identities used by the source shadow specialization cache.
/// Cache entries must retain both Arc owners to prevent address reuse.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SpecializationKey {
    mesh_bits: u64,
    layout: usize,
    properties: usize,
}
impl SpecializationKey {
    pub(super) fn new<L, P>(
        mesh_bits: u64,
        layout: &std::sync::Arc<L>,
        properties: &std::sync::Arc<P>,
    ) -> Self {
        Self {
            mesh_bits,
            layout: std::sync::Arc::as_ptr(layout) as usize,
            properties: std::sync::Arc::as_ptr(properties) as usize,
        }
    }
}
