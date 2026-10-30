//! Shadow batches share material state only when the checked caster ignores it.

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShadowBatchKey<P, D, M> {
    pub pipeline: P,
    pub draw: D,
    pub material: Option<M>,
    pub split_mask: u8,
    pub indexed: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CasterBatchState {
    pub standard: bool,
    pub opaque: bool,
    pub flags: u32,
    pub lod_fade: f32,
}
impl CasterBatchState {
    /// SpecularMaterial validates its opaque blend and depth state when constructed.
    pub fn opaque_specular(flags: u32, lod_fade: f32) -> Self {
        Self {
            standard: true,
            opaque: true,
            flags,
            lod_fade,
        }
    }

    pub fn material_key<M>(
        self,
        material: M,
        revision: u64,
        prepared_revision: Option<u64>,
    ) -> Option<M> {
        // source_caster.wgsl reads alpha for bits 8 and 9. Also exclude alpha blending.
        const ALPHA_FLAGS: u32 = (1 << 8) | (1 << 9) | (1 << 10);
        if self.standard
            && self.opaque
            && self.flags & ALPHA_FLAGS == 0
            && self.lod_fade == 1.
            && prepared_revision == Some(revision)
        {
            None
        } else {
            Some(material)
        }
    }
}
