//! Retain world bounds without changing the caller's transform arithmetic or entity order.
use super::shadow_candidates::Bounds;
use std::{collections::HashMap, hash::Hash};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BoundsKey {
    transform: [u32; 12],
    center: [u32; 3],
    extents: [u32; 3],
}
impl BoundsKey {
    pub fn new(transform: [f32; 12], center: [f32; 3], extents: [f32; 3]) -> Self {
        Self {
            transform: transform.map(f32::to_bits),
            center: center.map(f32::to_bits),
            extents: extents.map(f32::to_bits),
        }
    }
}
struct Row {
    key: BoundsKey,
    bounds: Bounds,
}
pub struct WorldBoundsCache<K> {
    rows: HashMap<K, Row>,
    epoch: Option<(u64, u32, [u64; 3])>,
}
impl<K> Default for WorldBoundsCache<K> {
    fn default() -> Self {
        Self {
            rows: HashMap::new(),
            epoch: None,
        }
    }
}
impl<K: Eq + Hash> WorldBoundsCache<K> {
    pub fn reserve(&mut self, count: usize) {
        self.rows.reserve(count.saturating_sub(self.rows.len()));
    }
    /// Scene and origin changes invalidate every retained bound.
    pub fn begin(&mut self, session: u64, scene: u32, origin: [f64; 3]) -> bool {
        let epoch = (session, scene, origin.map(f64::to_bits));
        let changed = self.epoch != Some(epoch);
        if changed {
            self.rows.clear();
        }
        self.epoch = Some(epoch);
        changed
    }
    pub fn get_or_update(
        &mut self,
        entity: K,
        key: BoundsKey,
        calculate: impl FnOnce() -> Bounds,
    ) -> Result<Bounds, &'static str> {
        if let Some(row) = self.rows.get(&entity) {
            if row.key == key {
                return Ok(row.bounds);
            }
        }
        let bounds = calculate();
        if bounds
            .center
            .iter()
            .chain(bounds.extents.iter())
            .any(|value| !value.is_finite())
            || bounds.extents.iter().any(|value| *value < 0.)
        {
            return Err("Source shadow renderer bounds are nonfinite");
        }
        self.rows.insert(entity, Row { key, bounds });
        Ok(bounds)
    }
    /// Membership changes rebuild the complete ordered collection.
    pub fn clear(&mut self) {
        self.rows.clear();
    }
}
