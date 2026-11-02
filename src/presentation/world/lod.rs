//! Checked generated LODGroup membership. Runtime coefficients come from the source culling path.
use crate::reference::{
    ReferenceError, SourceId, WorldPrototypeReference, WorldReference, WorldValue,
};
use bevy::math::{Mat4, Vec3};
#[derive(Clone, Copy)]
pub struct LodView {
    pub position: Vec3,
    pub fov_degrees: f32,
    pub ortho_size: Option<f32>,
}
impl LodView {
    /// Checked perspective camera factor: scaled distance per camera distance.
    pub fn factor(&self, bias: f32) -> f32 {
        let angle = self.fov_degrees / 360.0;
        let angle = angle + angle;
        let angle = angle * std::f32::consts::PI;
        let angle = angle * 0.5;
        ((angle.tan() as f64) * 2.0) as f32 / bias
    }
    fn distance(&self, center: Vec3, bias: f32) -> f32 {
        if let Some(half_height) = self.ortho_size {
            2.0 * half_height / bias
        } else {
            let factor = self.factor(bias);
            ((self.position - center).length_squared() * (factor * factor)).sqrt()
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
pub struct LodSelection {
    pub first: usize,
    pub fade: f32,
    pub transition: bool,
}
pub struct LodLevel {
    pub height: f32,
    pub width: f32,
    pub nodes: Vec<usize>,
}
pub struct LodGroup {
    pub source: SourceId,
    pub node: usize,
    pub point: Vec3,
    pub size: f32,
    pub levels: Vec<LodLevel>,
}
fn link(value: Option<&WorldValue>) -> Option<&SourceId> {
    match value {
        Some(WorldValue::Link(value)) => Some(value),
        _ => None,
    }
}
fn number(value: Option<&WorldValue>) -> Result<f32, ReferenceError> {
    match value {
        Some(WorldValue::Number(value)) => Ok(*value as f32),
        Some(WorldValue::Integer(value)) => Ok(*value as f32),
        _ => Err(ReferenceError("Missing checked LOD number".into())),
    }
}
impl LodGroup {
    fn for_prototype<'a>(
        reference: &WorldReference,
        prototype: &WorldPrototypeReference,
        owner: impl Fn(&SourceId) -> Option<&'a SourceId>,
    ) -> Result<Vec<Self>, ReferenceError> {
        let node_for = |node_owner: &SourceId| {
            prototype
                .nodes
                .iter()
                .position(|n| owner(&n.source) == Some(node_owner))
        };
        let mut result = Vec::new();
        for record in reference
            .records
            .iter()
            .filter(|r| r.class_name == "LODGroup")
        {
            let Some(node) = owner(&record.source).and_then(node_for) else {
                continue;
            };
            if matches!(
                record.fields.get("m_Enabled"),
                Some(WorldValue::Bool(false))
            ) {
                continue;
            }
            if !matches!(
                record.fields.get("m_FadeMode"),
                Some(WorldValue::Integer(1))
            ) || !matches!(
                record.fields.get("m_AnimateCrossFading"),
                Some(WorldValue::Bool(false))
            ) {
                return Err(ReferenceError(
                    "Unsupported source LOD transition mode".into(),
                ));
            }
            let Some(WorldValue::Record(point)) = record.fields.get("m_LocalReferencePoint") else {
                return Err(ReferenceError("Missing source LOD reference point".into()));
            };
            let point = Vec3::new(
                number(point.get("x"))?,
                number(point.get("y"))?,
                -number(point.get("z"))?,
            );
            let Some(WorldValue::Array(entries)) = record.fields.get("m_LODs") else {
                return Err(ReferenceError("Missing source LOD levels".into()));
            };
            let mut levels = Vec::with_capacity(entries.len());
            for entry in entries {
                let WorldValue::Record(fields) = entry else {
                    return Err(ReferenceError("Invalid source LOD level".into()));
                };
                let Some(WorldValue::Array(renderers)) = fields.get("renderers") else {
                    return Err(ReferenceError("Missing LOD renderer membership".into()));
                };
                let mut nodes = Vec::with_capacity(renderers.len());
                for renderer in renderers {
                    let WorldValue::Record(fields) = renderer else {
                        return Err(ReferenceError("Invalid LOD renderer entry".into()));
                    };
                    if let Some(source) = link(fields.get("renderer")) {
                        let index = owner(source).and_then(node_for).ok_or_else(|| {
                            ReferenceError("LOD renderer lies outside its prototype".into())
                        })?;
                        nodes.push(index);
                    }
                }
                levels.push(LodLevel {
                    height: number(fields.get("screenRelativeHeight"))?,
                    width: number(fields.get("fadeTransitionWidth"))?,
                    nodes,
                });
            }
            result.push(Self {
                source: record.source.clone(),
                node,
                point,
                size: number(record.fields.get("m_Size"))?,
                levels,
            });
        }
        Ok(result)
    }
    /// World center and size of the group for a node world matrix.
    pub fn bounds(&self, world: Mat4) -> (Vec3, f32) {
        let scale = Vec3::new(
            world.x_axis.truncate().length(),
            world.y_axis.truncate().length(),
            world.z_axis.truncate().length(),
        );
        (
            world.transform_point3(self.point),
            scale.max_element() * self.size,
        )
    }
    /// UnityPlayer 3f99f0 selection and checked camera factor, and checked static width coefficients.
    pub fn select(
        &self,
        world: Mat4,
        view: LodView,
        bias: f32,
        maximum: usize,
    ) -> Option<LodSelection> {
        let (center, size) = self.bounds(world);
        self.select_at(center, size, view, bias, maximum)
    }
    pub fn select_at(
        &self,
        center: Vec3,
        size: f32,
        view: LodView,
        bias: f32,
        maximum: usize,
    ) -> Option<LodSelection> {
        let distance = view.distance(center, bias);
        for (index, level) in self.levels.iter().enumerate().skip(maximum) {
            let threshold = size / level.height;
            if distance >= threshold {
                continue;
            }
            let width = self.width(index, size, threshold);
            let remaining = threshold - distance;
            let transition = width > 0.0 && remaining < width;
            return Some(LodSelection {
                first: index,
                fade: if transition { remaining / width } else { 1.0 },
                transition,
            });
        }
        None
    }
    fn width(&self, index: usize, size: f32, threshold: f32) -> f32 {
        let level = &self.levels[index];
        let previous = if index == 0 {
            1.0
        } else {
            self.levels[index - 1].height
        };
        if size.abs() <= 1.0e-6 || level.width <= 1.0e-5 {
            0.0
        } else {
            (threshold - size / (level.height + (previous - level.height) * level.width)).max(0.0)
        }
    }
    /// Camera travel that cannot change `select_at`. Zero during a fade transition.
    /// The margin covers f32 rounding of the distance and the boundaries.
    pub fn safe_distance(
        &self,
        center: Vec3,
        size: f32,
        view: LodView,
        bias: f32,
        maximum: usize,
    ) -> f32 {
        let distance = view.distance(center, bias);
        let mut selected = false;
        let mut safe = f32::INFINITY;
        for (index, level) in self.levels.iter().enumerate().skip(maximum) {
            let threshold = size / level.height;
            let width = self.width(index, size, threshold);
            if !selected && distance < threshold {
                selected = true;
                if width > 0.0 && threshold - distance < width {
                    return 0.0;
                }
            }
            // Comparisons with a non-finite boundary never change.
            if !threshold.is_finite() {
                continue;
            }
            let error = 1.0e-5 * (2.0 * distance + threshold.abs());
            safe = safe.min((distance - threshold).abs() - error);
            if width > 0.0 {
                safe = safe.min((distance - (threshold - width)).abs() - error);
            }
        }
        if view.ortho_size.is_some() || safe == f32::INFINITY {
            return f32::INFINITY;
        }
        if !distance.is_finite() {
            return 0.0;
        }
        (safe / view.factor(bias).abs()).max(0.0)
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.source.file.capacity()
            + self.levels.capacity() * std::mem::size_of::<LodLevel>()
            + self
                .levels
                .iter()
                .map(|l| l.nodes.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
    }
}
/// Per prototype LOD groups and renderer membership. Built once per loaded prototype.
pub struct LodTable {
    groups: Vec<LodGroup>,
    // glTF node of each group, which gives the instance world matrix.
    worlds: Vec<usize>,
    // Range in `levels` for each prototype node.
    ranges: Vec<std::ops::Range<u32>>,
    // (group, level) pairs: the first level of each group that holds the node.
    levels: Vec<(u32, u32)>,
}
impl LodTable {
    pub fn new(groups: Vec<LodGroup>, worlds: Vec<usize>, nodes: usize) -> Self {
        let mut ranges = Vec::with_capacity(nodes);
        let mut levels = Vec::new();
        for node in 0..nodes {
            let start = levels.len() as u32;
            for (index, group) in groups.iter().enumerate() {
                if let Some(level) = group.levels.iter().position(|l| l.nodes.contains(&node)) {
                    levels.push((index as u32, level as u32));
                }
            }
            ranges.push(start..levels.len() as u32);
        }
        Self {
            groups,
            worlds,
            ranges,
            levels,
        }
    }
    pub fn for_prototype<'a>(
        reference: &WorldReference,
        prototype: &WorldPrototypeReference,
        owner: impl Fn(&SourceId) -> Option<&'a SourceId>,
    ) -> Result<Self, ReferenceError> {
        let groups = LodGroup::for_prototype(reference, prototype, owner)?;
        let worlds = groups
            .iter()
            .map(|g| prototype.nodes[g.node].gltf_node as usize)
            .collect();
        Ok(Self::new(groups, worlds, prototype.nodes.len()))
    }
    /// The (group, level) pairs of a prototype node.
    pub fn levels(&self, node: usize) -> &[(u32, u32)] {
        let range = &self.ranges[node];
        &self.levels[range.start as usize..range.end as usize]
    }
}
#[derive(Clone, Copy)]
struct GroupState {
    center: Vec3,
    size: f32,
    selection: Option<LodSelection>,
    // Travel at which the group evaluates again. NaN when no entry is queued.
    deadline: f64,
}
/// Per instance LOD state. Renderers read the group results.
pub struct LodInstance {
    table: std::sync::Arc<LodTable>,
    groups: Vec<GroupState>,
    stamp: u64,
    queued: bool,
    changed: bool,
}
impl LodInstance {
    pub fn new(table: std::sync::Arc<LodTable>) -> Self {
        let state = GroupState {
            center: Vec3::ZERO,
            size: 0.0,
            selection: None,
            deadline: f64::NAN,
        };
        Self {
            groups: vec![state; table.groups.len()],
            table,
            stamp: 0,
            queued: false,
            changed: false,
        }
    }
    pub fn table(&self) -> &LodTable {
        &self.table
    }
    /// Renderer (visible, fade) of a prototype node from the group results.
    pub fn renderer(&self, node: usize) -> (bool, f32) {
        let mut visible = true;
        let mut fade = 1.0;
        for &(group, level) in self.table.levels(node) {
            let level = level as usize;
            match self.groups[group as usize].selection {
                Some(value) if level == value.first => {
                    fade = value.fade;
                }
                Some(value) if value.transition && level == value.first + 1 => {
                    fade = value.fade - 1.0;
                }
                _ => {
                    visible = false;
                }
            }
        }
        (visible, fade)
    }
}
/// An instance that holds LOD state. Retired instances return `None`.
pub trait LodOwner {
    /// LOD state and the world matrices of the instance glTF nodes.
    fn lod(&mut self) -> Option<(&mut LodInstance, &[bevy::math::DMat4])>;
}
const ALL: u32 = u32::MAX;
// FOV, ortho size, bias, maximum and world origin. A change evaluates every group.
type ViewKey = (f32, Option<f32>, f32, usize, Option<[f64; 3]>);
struct Due {
    deadline: f64,
    slot: u32,
    group: u32,
    stamp: u64,
}
impl PartialEq for Due {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Due {}
impl PartialOrd for Due {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Due {
    // Earliest deadline first.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.deadline.total_cmp(&self.deadline)
    }
}
/// Evaluates a group only when the camera path since its last evaluation exceeds its safe distance.
/// Results equal `LodGroup::select` for every group on every update.
#[derive(Default)]
pub struct LodSchedule {
    // Camera path length. Bounds the camera distance from each evaluation point.
    travel: f64,
    camera: Option<Vec3>,
    key: Option<ViewKey>,
    view: Option<(LodView, f32, usize)>,
    due: std::collections::BinaryHeap<Due>,
    stamps: u64,
    // Groups plus one per scheduled instance. Limits stale queue entries.
    live: usize,
    evaluated: usize,
}
impl LodSchedule {
    /// Groups evaluated by the last update.
    pub fn evaluated(&self) -> usize {
        self.evaluated
    }
    pub fn retained_bytes(&self) -> usize {
        self.due.capacity() * std::mem::size_of::<Due>()
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Starts a new instance in `slot`. It evaluates at the next update.
    pub fn spawn(&mut self, lod: &mut LodInstance, slot: usize) {
        self.live += lod.groups.len() + 1;
        self.restamp(lod, slot);
    }
    pub fn remove(&mut self, lod: &LodInstance) {
        self.live -= lod.groups.len() + 1;
    }
    /// The instance moved to another slot. It evaluates at the next update.
    pub fn relocate(&mut self, lod: &mut LodInstance, slot: usize) {
        self.restamp(lod, slot);
    }
    fn restamp(&mut self, lod: &mut LodInstance, slot: usize) {
        self.stamps += 1;
        lod.stamp = self.stamps;
        lod.queued = false;
        for group in &mut lod.groups {
            group.deadline = f64::NAN;
        }
        self.queue(lod, slot);
    }
    /// Evaluates every group of the instance and applies its renderers at the next update.
    /// Use it when the instance world matrices or renderer states change.
    pub fn queue(&mut self, lod: &mut LodInstance, slot: usize) {
        if lod.queued {
            return;
        }
        lod.queued = true;
        self.due.push(Due {
            deadline: f64::NEG_INFINITY,
            slot: slot as u32,
            group: ALL,
            stamp: lod.stamp,
        });
    }
    /// Evaluates due groups. `apply` runs for each instance with changed group results.
    pub fn update<I: LodOwner>(
        &mut self,
        view: LodView,
        bias: f32,
        maximum: usize,
        origin: Option<[f64; 3]>,
        owners: &mut [I],
        mut apply: impl FnMut(&mut I),
    ) {
        self.evaluated = 0;
        self.view = Some((view, bias, maximum));
        let key = (view.fov_degrees, view.ortho_size, bias, maximum, origin);
        let step = self.camera.map_or(0.0, |camera| {
            (view.position.as_dvec3() - camera.as_dvec3()).length()
        });
        self.camera = Some(view.position);
        if self.key != Some(key) || !step.is_finite() {
            // The camera factor or frame changed: evaluate every group.
            self.key = Some(key);
            self.due.clear();
            for (slot, owner) in owners.iter_mut().enumerate() {
                let Some((lod, worlds)) = owner.lod() else {
                    continue;
                };
                lod.queued = false;
                for group in &mut lod.groups {
                    group.deadline = f64::NAN;
                }
                self.evaluate_all(slot, lod, worlds);
                lod.changed = false;
                apply(owner);
            }
            return;
        }
        self.travel += step;
        while self.due.peek().is_some_and(|d| d.deadline < self.travel) {
            let due = self.due.pop().unwrap();
            let Some(owner) = owners.get_mut(due.slot as usize) else {
                continue;
            };
            {
                let Some((lod, worlds)) = owner.lod() else {
                    continue;
                };
                if lod.stamp != due.stamp {
                    continue;
                }
                if due.group == ALL {
                    if !lod.queued {
                        continue;
                    }
                    lod.queued = false;
                    self.evaluate_all(due.slot as usize, lod, worlds);
                } else {
                    let group = due.group as usize;
                    if lod.groups[group].deadline.to_bits() != due.deadline.to_bits() {
                        continue;
                    }
                    self.evaluate(due.slot as usize, lod, group);
                }
                if !std::mem::take(&mut lod.changed) {
                    continue;
                }
            }
            apply(owner);
        }
        if self.due.len() > 2 * self.live + 64 {
            // Drop entries of moved, removed or retired instances.
            self.due.retain(|due| {
                owners
                    .get_mut(due.slot as usize)
                    .and_then(|owner| owner.lod())
                    .is_some_and(|(lod, _)| {
                        lod.stamp == due.stamp
                            && if due.group == ALL {
                                lod.queued
                            } else {
                                lod.groups[due.group as usize].deadline.to_bits()
                                    == due.deadline.to_bits()
                            }
                    })
            });
        }
    }
    fn evaluate_all(&mut self, slot: usize, lod: &mut LodInstance, worlds: &[bevy::math::DMat4]) {
        for group in 0..lod.groups.len() {
            let world = worlds[lod.table.worlds[group]].as_mat4();
            let (center, size) = lod.table.groups[group].bounds(world);
            lod.groups[group].center = center;
            lod.groups[group].size = size;
            self.evaluate(slot, lod, group);
        }
        lod.changed = true;
    }
    fn evaluate(&mut self, slot: usize, lod: &mut LodInstance, group: usize) {
        let (view, bias, maximum) = self.view.unwrap();
        let source = &lod.table.groups[group];
        let state = &mut lod.groups[group];
        let selection = source.select_at(state.center, state.size, view, bias, maximum);
        let safe = source.safe_distance(state.center, state.size, view, bias, maximum) as f64;
        self.evaluated += 1;
        if state.selection != selection {
            state.selection = selection;
            lod.changed = true;
        }
        // Round the deadline down against f64 rounding of the travel sum.
        let deadline = self.travel + (safe - self.travel * 4.0 * f64::EPSILON).max(0.0);
        if deadline.to_bits() == state.deadline.to_bits() {
            return;
        }
        state.deadline = deadline;
        if deadline.is_finite() {
            self.due.push(Due {
                deadline,
                slot: slot as u32,
                group: group as u32,
                stamp: lod.stamp,
            });
        }
    }
}
/// Random LOD tables and instance placements for scheduler tests.
#[cfg(test)]
pub(super) mod fixture {
    use super::*;
    use bevy::math::{DMat4, DQuat, DVec3};
    use std::sync::Arc;
    pub struct Random(pub u64);
    impl Random {
        pub fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
        pub fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.next()
        }
        pub fn index(&mut self, count: usize) -> usize {
            ((self.next() * count as f32) as usize).min(count - 1)
        }
    }
    pub const NODES: usize = 8;
    pub fn table(random: &mut Random) -> Arc<LodTable> {
        let groups = (0..1 + random.index(3))
            .map(|_| {
                let mut height = random.range(0.3, 0.9);
                let levels = (0..1 + random.index(4))
                    .map(|_| {
                        height *= random.range(0.1, 0.7);
                        LodLevel {
                            height,
                            // Wide fades give many transition frames.
                            width: if random.next() < 0.2 {
                                0.0
                            } else {
                                random.next()
                            },
                            // Some nodes sit in two levels of one group.
                            nodes: (0..1 + random.index(3))
                                .map(|_| random.index(NODES))
                                .collect(),
                        }
                    })
                    .collect();
                LodGroup {
                    source: SourceId {
                        file: String::new(),
                        path_id: 0,
                    },
                    node: 0,
                    point: Vec3::new(random.range(-2.0, 2.0), random.range(0.0, 3.0), 0.0),
                    size: random.range(0.5, 12.0),
                    levels,
                }
            })
            .collect::<Vec<_>>();
        let worlds = groups.iter().map(|_| random.index(NODES)).collect();
        Arc::new(LodTable::new(groups, worlds, NODES))
    }
    pub fn place(random: &mut Random, worlds: &mut [DMat4]) {
        let base = DVec3::new(
            random.range(-600.0, 600.0) as f64,
            random.range(-20.0, 20.0) as f64,
            random.range(-600.0, 600.0) as f64,
        );
        for world in worlds {
            *world = DMat4::from_scale_rotation_translation(
                DVec3::splat(random.range(0.5, 2.0) as f64),
                DQuat::from_rotation_y(random.range(0.0, 6.0) as f64),
                base + DVec3::new(random.range(-5.0, 5.0) as f64, 0.0, 0.0),
            );
        }
    }
}
