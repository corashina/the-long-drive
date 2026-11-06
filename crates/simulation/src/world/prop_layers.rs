//! Source object layers retain candidate buffers only for active chunks.
use super::{Placement, PropCandidate, PropGenerator, TerrainContext};
use crate::reference::{ReferenceError, WorldReference};
use std::collections::{HashMap, HashSet};

/// Layer, chunk unit and ordinal of a placement: the part that its children share.
pub(super) type RootKey = (super::WorldLayer, [i64; 2], u32);
pub(super) fn root_key(id: &super::GeneratedId) -> RootKey {
    (id.layer, id.unit, id.ordinal)
}

/// Chunk candidates by ordinal. Empty when no candidate is left.
type Slot = Option<PropCandidate>;
struct Chunk {
    layer: usize,
    unit: [i64; 2],
    candidates: Vec<Slot>,
    left: usize,
    placed: Vec<super::GeneratedId>,
}
/// What one `advance` changed in the placements.
#[derive(Default)]
pub struct PropChange {
    /// The removed placement id, and true if no placement has its root key now.
    pub removed: Option<(super::GeneratedId, bool)>,
    /// True if the last placement is new.
    pub added: bool,
}
struct Layer {
    generator: PropGenerator,
    /// Count of active chunks that still hold each candidate ordinal.
    held: Vec<u32>,
    offsets: Vec<[f32; 2]>,
    centers: [[f32; 2]; 2],
    next: [usize; 2],
    focus: usize,
    place_focus: usize,
    remove_squared: f32,
}
pub struct PropLayers {
    layers: [Layer; 4],
    chunks: Vec<Chunk>,
    placements: Vec<Placement>,
    /// Ids and root keys of `placements`, for lookups without a scan.
    ids: HashSet<super::GeneratedId>,
    roots: HashMap<RootKey, u32>,
    retiring: Vec<super::GeneratedId>,
    layer: usize,
    /// Bumps when `placements` changes.
    revision: u64,
}
impl PropLayers {
    pub fn new(
        reference: &WorldReference,
        seed: i32,
        distances: &super::DrawDistances,
    ) -> Result<Self, ReferenceError> {
        let mut layers = Vec::with_capacity(4);
        for i in 0..4 {
            let generator = PropGenerator::new(reference, seed, i)?;
            let (draw, remove) = layer_distances(distances, i);
            let size = generator.reference.chunk_dist;
            let side = ((draw * 2.0 / size) as usize + 1).max(2);
            let half = (side - 1) as f32 * size * 0.5;
            let mut offsets = Vec::with_capacity(side * side);
            for z in 0..side {
                for x in 0..side {
                    offsets.push([x as f32 * size - half, z as f32 * size - half]);
                }
            }
            offsets.sort_by(|a, b| squared(*a).total_cmp(&squared(*b)));
            layers.push(Layer {
                held: vec![0; generator.candidate_count()],
                generator,
                offsets,
                centers: [[f32::INFINITY; 2]; 2],
                next: [0; 2],
                focus: 0,
                place_focus: 0,
                remove_squared: remove * remove,
            });
        }
        Ok(Self {
            layers: layers.try_into().ok().unwrap(),
            chunks: Vec::new(),
            placements: Vec::new(),
            ids: HashSet::new(),
            roots: HashMap::new(),
            retiring: Vec::new(),
            layer: 0,
            revision: 0,
        })
    }
    pub fn selection_complete(&self) -> bool {
        self.layers.iter().all(|layer| {
            layer
                .next
                .into_iter()
                .all(|next| next == layer.offsets.len())
        }) && self.chunks.iter().all(|chunk| chunk.candidates.is_empty())
            && self.retiring.is_empty()
    }
    pub fn pending_requests(&self) -> usize {
        self.chunks
            .iter()
            .filter(|c| !c.candidates.is_empty())
            .count()
    }
    pub fn focus_changed(&self, before: [[f64; 3]; 2], after: [[f64; 3]; 2]) -> bool {
        self.layers.iter().any(|layer| {
            before.into_iter().zip(after).any(|(a, b)| {
                let size = layer.generator.reference.chunk_dist;
                let center = |p: [f64; 3]| {
                    [p[0] as f32, -p[2] as f32].map(|v| (v / size).round_ties_even() * size)
                };
                center(a) != center(b)
            })
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
    pub fn contains(&self, id: &super::GeneratedId) -> bool {
        self.ids.contains(id)
    }
    /// True if a placement shares the layer, unit and ordinal of `id`.
    pub fn contains_root(&self, id: &super::GeneratedId) -> bool {
        self.roots.contains_key(&root_key(id))
    }
    pub fn advance(
        &mut self,
        focus: [[f64; 3]; 2],
        context: TerrainContext<'_>,
        can_request: bool,
        max_objects: usize,
    ) -> Result<PropChange, ReferenceError> {
        let mut change = PropChange::default();
        let i = self.layer;
        self.layer = (self.layer + 1) % self.layers.len();
        let points = focus.map(|p| [p[0] as f32, -p[2] as f32]);
        let layer = &mut self.layers[i];
        let size = layer.generator.reference.chunk_dist;
        for (j, point) in points.iter().enumerate() {
            let center = point.map(|v| (v / size).round_ties_even() * size);
            if center != layer.centers[j] {
                layer.centers[j] = center;
                layer.next[j] = 0;
            }
        }
        // The source retires one chunk, then one queued object, per layer update.
        if let Some(index) = self.chunks.iter().position(|c| {
            c.layer == i
                && points.iter().all(|p| {
                    squared([c.unit[0] as f32 - p[0], c.unit[1] as f32 - p[1]])
                        >= layer.remove_squared
                })
        }) {
            let chunk = self.chunks.remove(index);
            for (held, slot) in layer.held.iter_mut().zip(&chunk.candidates) {
                *held -= slot.is_some() as u32;
            }
            self.retiring.extend(chunk.placed);
        }
        if let Some(index) = self
            .retiring
            .iter()
            .position(|id| id.layer == layer.generator.layer)
        {
            let id = self.retiring.remove(index);
            // Compare the cheap fields first.
            if let Some(position) = self
                .placements
                .iter()
                .position(|p| p.id.ordinal == id.ordinal && p.id.unit == id.unit && p.id == id)
            {
                self.placements.remove(position);
                self.revision = self.revision.wrapping_add(1);
                self.ids.remove(&id);
                let key = root_key(&id);
                let count = self.roots.get_mut(&key).expect("a placement has a root");
                *count -= 1;
                let gone = *count == 0;
                if gone {
                    self.roots.remove(&key);
                }
                change.removed = Some((id, gone));
            }
        }
        let j = layer.focus;
        layer.focus = (layer.focus + 1) % 2;
        if can_request && layer.next[j] < layer.offsets.len() {
            let offset = layer.offsets[layer.next[j]];
            let unit = std::array::from_fn(|a| (layer.centers[j][a] + offset[a]) as i64);
            if !self.chunks.iter().any(|c| c.layer == i && c.unit == unit) {
                self.chunks
                    .try_reserve_exact(1)
                    .map_err(|_| ReferenceError("Source prop chunk allocation failed".into()))?;
                let count = layer.generator.candidate_count();
                let mut candidates = Vec::with_capacity(count);
                let mut left = 0;
                for ordinal in 0..count {
                    let slot = layer.generator.candidate(unit, ordinal);
                    if slot.is_some() {
                        layer.held[ordinal] += 1;
                        left += 1;
                    }
                    candidates.push(slot);
                }
                if left == 0 {
                    candidates = Vec::new();
                }
                self.chunks.push(Chunk {
                    layer: i,
                    unit,
                    candidates,
                    left,
                    placed: Vec::new(),
                });
            }
            layer.next[j] += 1;
        }
        let focus = points[layer.place_focus];
        layer.place_focus = (layer.place_focus + 1) % 2;
        if let Some((chunk_index, ordinal)) = nearest(
            &self.chunks,
            i,
            &layer.held,
            layer.generator.points(),
            focus,
        ) {
            self.placements
                .try_reserve_exact(1)
                .map_err(|_| ReferenceError("Source prop placement allocation failed".into()))?;
            let chunk = &mut self.chunks[chunk_index];
            chunk
                .placed
                .try_reserve_exact(1)
                .map_err(|_| ReferenceError("Source prop identity allocation failed".into()))?;
            let candidate = chunk.candidates[ordinal].expect("a held candidate");
            if let Some(placement) = layer.generator.place(chunk.unit, candidate, context) {
                if self.placements.len() >= max_objects {
                    return Err(ReferenceError(
                        "Source prop count exceeds object admission budget".into(),
                    ));
                }
                self.ids
                    .try_reserve(1)
                    .and(self.roots.try_reserve(1))
                    .map_err(|_| ReferenceError("Source prop index allocation failed".into()))?;
                chunk.placed.push(placement.id.clone());
                self.ids.insert(placement.id.clone());
                *self.roots.entry(root_key(&placement.id)).or_insert(0) += 1;
                self.placements.push(placement);
                self.revision = self.revision.wrapping_add(1);
                change.added = true;
            }
            chunk.candidates[ordinal] = None;
            layer.held[ordinal] -= 1;
            chunk.left -= 1;
            if chunk.left == 0 {
                chunk.candidates = Vec::new();
            }
        }
        Ok(change)
    }
    pub fn material(
        &self,
        placement: &Placement,
    ) -> Option<(&crate::reference::SourceId, &crate::reference::SourceId)> {
        self.layers
            .iter()
            .find(|l| l.generator.layer == placement.id.layer)?
            .generator
            .material(&placement.prototype, placement.object_seed)
    }
}
/// Generation and removal distance of prop layer `i`.
pub(super) fn layer_distances(distances: &super::DrawDistances, i: usize) -> (f32, f32) {
    match i {
        0 => (distances.obj_gen_dist, distances.obj_remove_dist),
        1 => (distances.mountain_gen_dist, distances.mountain_remove_dist),
        _ => (distances.building_gen_dist, distances.building_remove_dist),
    }
}
/// Source nearest candidate of layer `layer`: the first in chunk order, then ordinal order,
/// with the least distance from `focus` to its local point. The scan visits each ordinal once,
/// not each candidate of each chunk. All chunks share the local points.
fn nearest(
    chunks: &[Chunk],
    layer: usize,
    held: &[u32],
    points: &[[f32; 2]],
    focus: [f32; 2],
) -> Option<(usize, usize)> {
    // Source compares local Npos. Do not add the chunk origin here.
    let distance = |ordinal: usize| {
        let p = points[ordinal];
        squared([focus[0] - p[0], focus[1] - p[1]])
    };
    let mut best = None;
    let mut least = f32::MAX;
    let mut tied = false;
    for (ordinal, _) in held.iter().enumerate().filter(|(_, count)| **count > 0) {
        let d = distance(ordinal);
        if d < least {
            least = d;
            best = Some(ordinal);
            tied = false;
        } else if d == least {
            tied = true;
        }
    }
    let best = best?;
    let mut active = chunks.iter().enumerate().filter(|(_, c)| c.layer == layer);
    if !tied {
        return active
            .find(|(_, c)| c.candidates.get(best).is_some_and(Option::is_some))
            .map(|(index, _)| (index, best));
    }
    active.find_map(|(index, c)| {
        c.candidates
            .iter()
            .enumerate()
            .find(|(ordinal, slot)| slot.is_some() && distance(*ordinal) == least)
            .map(|(ordinal, _)| (index, ordinal))
    })
}
fn squared(p: [f32; 2]) -> f32 {
    p[0] * p[0] + p[1] * p[1]
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The old scan: every candidate of every chunk, first strict minimum wins.
    fn scan(chunks: &[Chunk], layer: usize, focus: [f32; 2]) -> Option<(usize, usize)> {
        let mut nearest = None;
        let mut distance = f32::MAX;
        for (index, chunk) in chunks.iter().enumerate().filter(|(_, c)| c.layer == layer) {
            for (ordinal, c) in chunk.candidates.iter().enumerate() {
                let Some(c) = c else { continue };
                let d = squared([focus[0] - c.local[0], focus[1] - c.local[1]]);
                if d < distance {
                    distance = d;
                    nearest = Some((index, ordinal));
                }
            }
        }
        nearest
    }

    #[test]
    fn nearest_matches_the_full_scan() {
        // A grid gives many equal distances, so ties are tested too.
        let points: Vec<[f32; 2]> = (0..36)
            .map(|i| [(i % 6) as f32 * 2.0, (i / 6) as f32 * 2.0])
            .collect();
        let mut state = 7u32;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 8
        };
        for _ in 0..500 {
            let mut held = vec![0; points.len()];
            let chunks: Vec<Chunk> = (0..6)
                .map(|n| {
                    let candidates: Vec<Slot> = points
                        .iter()
                        .enumerate()
                        .map(|(ordinal, &local)| {
                            (random() % 4 == 0).then_some(PropCandidate {
                                point: local,
                                local,
                                ordinal: ordinal as u32,
                                selected: 0,
                                rotation: 0,
                                random: 0,
                            })
                        })
                        .collect();
                    let layer = n % 2;
                    if layer == 0 {
                        for (h, slot) in held.iter_mut().zip(&candidates) {
                            *h += slot.is_some() as u32;
                        }
                    }
                    Chunk {
                        layer,
                        unit: [n as i64, 0],
                        left: candidates.iter().flatten().count(),
                        candidates,
                        placed: Vec::new(),
                    }
                })
                .collect();
            let focus = [(random() % 13) as f32, (random() % 13) as f32];
            assert_eq!(
                nearest(&chunks, 0, &held, &points, focus),
                scan(&chunks, 0, focus)
            );
        }
    }
}
