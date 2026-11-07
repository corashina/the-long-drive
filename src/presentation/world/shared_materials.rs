//! Scene-local immutable material bindings. Each admitted renderer owns one user.
use crate::reference::{AlphaMode, AssetId, MaterialRecord, ShaderFamily, SourceId};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MaterialIdentity {
    Primary(AssetId, u32),
    Specular(SourceId),
    /// Only GPU prop rows share impostors. Rows hold the fade and the source position.
    Impostor(SourceId),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SharedMaterialKey {
    pub session: u64,
    pub material: MaterialIdentity,
    pub layer: u8,
    pub receive: bool,
    pub reflected: bool,
    /// GPU prop rows use their own material. Its pipeline reads the row buffers.
    pub gpu: bool,
    /// Entity condition color as sRGBA bits. Entities with the same color share one material.
    pub color: Option<[u32; 4]>,
}
pub(crate) fn shareable_primary(record: &MaterialRecord) -> bool {
    matches!(
        record.shader_family,
        ShaderFamily::Standard | ShaderFamily::UnlitColor
    ) && matches!(record.state.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask)
        && record.state.z_write
        && record.state.z_test == 4
        && record.state.src_blend == 1
        && record.state.dst_blend == 0
}

pub(crate) struct SharedMaterials<T> {
    entries: HashMap<SharedMaterialKey, (T, usize)>,
}
impl<T> Default for SharedMaterials<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}
impl<T> Clone for SharedMaterials<T> {
    fn clone(&self) -> Self {
        // Save candidates own their bindings and GPU preparation state independently.
        Self::default()
    }
}
impl<T: Clone> SharedMaterials<T> {
    pub fn acquire(&mut self, key: &SharedMaterialKey) -> Option<T> {
        let (binding, users) = self.entries.get_mut(key)?;
        *users += 1;
        Some(binding.clone())
    }
    pub fn insert(&mut self, key: SharedMaterialKey, binding: T) {
        assert!(self.entries.insert(key, (binding, 1)).is_none());
    }
    fn remove(&mut self, key: &SharedMaterialKey) -> T {
        self.entries.remove(key).unwrap().0
    }
    pub fn make_private(&mut self, key: &SharedMaterialKey, copy: impl FnOnce(&T) -> T) -> T {
        if self.entries[key].1 == 1 {
            return self.remove(key);
        }
        let (binding, users) = self.entries.get_mut(key).unwrap();
        *users -= 1;
        copy(binding)
    }
    /// The last user takes the existing handles. Other users require a private copy.
    pub fn release(&mut self, key: &SharedMaterialKey) -> bool {
        let (_, users) = self
            .entries
            .get_mut(key)
            .expect("owned shared material binding");
        *users -= 1;
        if *users == 0 {
            self.remove(key);
            true
        } else {
            false
        }
    }
}
