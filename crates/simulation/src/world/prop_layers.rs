//! Source object layers retain candidate buffers only for active chunks.
use super::{Placement, PropCandidate, PropGenerator, TerrainContext};
use crate::reference::{ReferenceError, WorldReference};
use std::collections::{HashMap, HashSet};

/// Layer, chunk unit and ordinal of a placement: the part that its children share.
type RootKey = (super::WorldLayer, [i64; 2], u32);
fn root_key(id: &super::GeneratedId) -> RootKey {
    (id.layer, id.unit, id.ordinal)
}

struct Chunk {
    layer: usize,
    unit: [i64; 2],
    candidates: Vec<PropCandidate>,
    placed: Vec<super::GeneratedId>,
}
struct Layer {
    generator: PropGenerator,
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
        })
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.chunks.capacity() * std::mem::size_of::<Chunk>()
            + self
                .chunks
                .iter()
                .map(|c| {
                    c.candidates.capacity() * std::mem::size_of::<PropCandidate>()
                        + c.placed.capacity() * std::mem::size_of::<super::GeneratedId>()
                        + c.placed
                            .iter()
                            .map(super::contents::identity_heap)
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self.placements.capacity() * std::mem::size_of::<Placement>()
            + self.index_bytes(0)
            + self.retiring.capacity() * std::mem::size_of::<super::GeneratedId>()
            + self
                .placements
                .iter()
                .map(super::contents::placement_heap)
                .sum::<usize>()
            + self
                .retiring
                .iter()
                .map(super::contents::identity_heap)
                .sum::<usize>()
            + self
                .layers
                .iter()
                .map(|l| l.offsets.capacity() * 8 + l.generator.retained_bytes())
                .sum::<usize>()
    }
    pub fn pending_bytes(&self) -> usize {
        self.chunks
            .iter()
            .map(|c| c.candidates.capacity() * std::mem::size_of::<PropCandidate>())
            .sum()
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
    /// Index bytes with room for `extra` more placements.
    fn index_bytes(&self, extra: usize) -> usize {
        super::metrics::map_bytes(
            self.ids.capacity().max(self.ids.len() + extra),
            std::mem::size_of::<super::GeneratedId>(),
        ) + self
            .ids
            .iter()
            .map(super::contents::identity_heap)
            .sum::<usize>()
            + super::metrics::map_bytes(
                self.roots.capacity().max(self.roots.len() + extra),
                std::mem::size_of::<(RootKey, u32)>(),
            )
    }
    /// Reserve a complete candidate buffer before any source generation work.
    pub fn allocation_bound(&self) -> usize {
        self.layers[self.layer].generator.candidate_count() * std::mem::size_of::<PropCandidate>()
            + (self.chunks.capacity() + 1) * std::mem::size_of::<Chunk>()
            + (self.placements.capacity() + 1) * std::mem::size_of::<Placement>()
            + self.index_bytes(1)
            + self
                .chunks
                .iter()
                .map(|c| (c.placed.capacity() + 1) * std::mem::size_of::<super::GeneratedId>())
                .max()
                .unwrap_or(0)
            + self
                .chunks
                .iter()
                .map(|c| c.placed.len())
                .max()
                .map_or(0, |count| {
                    if self.retiring.len() + count > self.retiring.capacity() {
                        (self.retiring.len() + count) * std::mem::size_of::<super::GeneratedId>()
                    } else {
                        0
                    }
                })
            + 2 * self.layers[self.layer].generator.retained_bytes()
    }
    pub fn advance(
        &mut self,
        focus: [[f64; 3]; 2],
        context: TerrainContext<'_>,
        can_request: bool,
        max_objects: usize,
    ) -> Result<(), ReferenceError> {
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
            self.retiring.extend(chunk.placed);
        }
        if let Some(index) = self
            .retiring
            .iter()
            .position(|id| id.layer == layer.generator.layer)
        {
            let id = self.retiring.remove(index);
            let before = self.placements.len();
            self.placements.retain(|p| p.id != id);
            let removed = (before - self.placements.len()) as u32;
            if removed > 0 {
                self.ids.remove(&id);
                let key = root_key(&id);
                let count = self.roots.get_mut(&key).expect("a placement has a root");
                *count -= removed;
                if *count == 0 {
                    self.roots.remove(&key);
                }
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
                let mut candidates = Vec::with_capacity(layer.generator.candidate_count());
                for ordinal in 0..layer.generator.candidate_count() {
                    if let Some(c) = layer.generator.candidate(unit, ordinal) {
                        candidates.push(c);
                    }
                }
                self.chunks.push(Chunk {
                    layer: i,
                    unit,
                    candidates,
                    placed: Vec::new(),
                });
            }
            layer.next[j] += 1;
        }
        let focus = points[layer.place_focus];
        layer.place_focus = (layer.place_focus + 1) % 2;
        let mut nearest = None;
        let mut distance = f32::MAX;
        for (chunk_index, chunk) in self.chunks.iter().enumerate().filter(|(_, c)| c.layer == i) {
            for (candidate_index, candidate) in chunk.candidates.iter().enumerate() {
                // Source compares local Npos. Do not add the chunk origin here.
                let d = squared([
                    focus[0] - (candidate.local[0]),
                    focus[1] - (candidate.local[1]),
                ]);
                if d < distance {
                    distance = d;
                    nearest = Some((chunk_index, candidate_index));
                }
            }
        }
        if let Some((chunk_index, candidate_index)) = nearest {
            self.placements
                .try_reserve_exact(1)
                .map_err(|_| ReferenceError("Source prop placement allocation failed".into()))?;
            let chunk = &mut self.chunks[chunk_index];
            chunk
                .placed
                .try_reserve_exact(1)
                .map_err(|_| ReferenceError("Source prop identity allocation failed".into()))?;
            let candidate = chunk.candidates[candidate_index];
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
            }
            chunk.candidates.remove(candidate_index);
            if chunk.candidates.is_empty() {
                chunk.candidates = Vec::new();
            }
        }
        Ok(())
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
fn squared(p: [f32; 2]) -> f32 {
    p[0] * p[0] + p[1] * p[1]
}
