//! The newest simulation frame. Bevy systems read the simulation only through it.
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use simulation::boundary::{
    Change, Feedback, Handle, ItemRow, MaterialRow, NodeColorRow, NodePoseRow, ParticleGroupRow,
    ParticleRow, PoseKey, PoseRow, RopeAim, SimFrame, SpawnData,
};
use simulation::conditions::ConditionState;
use simulation::reference::{ConditionId, ItemId, NodeId, SourceId};
use simulation::starting_area::spawned::SpawnedItem;
use simulation::vehicle::VehiclePose;
use simulation::world::{GeneratedCapVisual, GeneratedId};
use std::collections::HashMap;

/// One particle group of the frame: kind, kind ids and source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParticleGroupKey {
    pub kind: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub source: u32,
}

/// The last view of one structure slot.
#[derive(Default)]
pub(crate) struct StructureView {
    id: Option<GeneratedId>,
    /// `StructureRow` flags.
    pub flags: u32,
    pub lamp: [f32; 4],
    pub display_map: u32,
    pub poses: Vec<NodePoseRow>,
    pub hidden: Vec<u32>,
    pub materials: Vec<MaterialRow>,
    pub colors: Vec<NodeColorRow>,
    pub caps: Vec<GeneratedCapVisual>,
}

#[derive(Resource, Default)]
pub(crate) struct SimFrameResource {
    pub frame: SimFrame,
    poses: HashMap<PoseKey, (Handle, SpawnData)>,
    rig: Vec<NodeId>,
    spawned: Vec<SpawnedItem>,
    particle_groups: HashMap<ParticleGroupKey, usize>,
    conditions: HashMap<ConditionId, ConditionState>,
    /// Owners whose condition changed since the visuals last read them.
    pub changed_conditions: Vec<ConditionId>,
    /// Home cable aims waiting for the reference visuals.
    pub home_cable: Option<Vec<RopeAim>>,
    /// Feedback waiting for the HUD.
    pub feedback: Option<Box<Feedback>>,
    structure_slots: HashMap<GeneratedId, u32>,
    /// Last view by structure slot.
    pub structure_views: Vec<StructureView>,
    /// Material sources by `MaterialRow::source`.
    pub material_sources: Vec<SourceId>,
    /// Slots whose view changed since the instances last read them.
    pub changed_structures: Vec<u32>,
    /// Screens to draw, oldest first. The computer screens take them.
    pub screens: Vec<simulation::interaction::computer::screen::ScreenView>,
    /// The new computer set, if it changed.
    pub screen_set: Option<Vec<ItemId>>,
    /// A structure took or left a slot since the instances last read them.
    pub structures_moved: bool,
    /// Plate texts by world car.
    pub plates: HashMap<GeneratedId, Vec<String>>,
    /// The simulation part of a world measurement, waiting for the renderer part.
    pub measurements: Option<String>,
    /// Sandbox replies that the browser has not taken.
    pub sandbox: Vec<String>,
    /// A simulation failure that the browser did not show yet.
    pub failure: Option<String>,
    /// Renderer credit of the last world step, for the next renderer step.
    pub grant: Option<simulation::world::PresentationGrant>,
}

impl SimFrameResource {
    /// Applies the frame changes to the pose index.
    pub fn apply_changes(&mut self) {
        for row in &self.frame.conditions {
            let id = ConditionId(row.id);
            self.conditions.insert(id, row.startup());
            self.changed_conditions.push(id);
        }
        for change in &self.frame.changes {
            match change {
                Change::Spawn { handle, key, data } => {
                    self.poses.insert(*key, (*handle, *data));
                    if let SpawnData::Spawned(item) = data {
                        self.spawned.push(*item);
                    }
                }
                Change::Retire(handle) => {
                    let spawned = &mut self.spawned;
                    self.poses.retain(|key, (h, _)| {
                        let keep = h != handle;
                        if !keep && let PoseKey::Item(id) = key {
                            spawned.retain(|item| item.id != *id);
                        }
                        keep
                    });
                }
                Change::PlayerRig(joints) => self.rig.clone_from(joints),
                Change::HomeCable(aims) => self.home_cable = Some(aims.clone()),
                Change::Feedback(feedback) => self.feedback = Some(feedback.clone()),
                Change::StructureAdd { slot, id } => {
                    let slot = *slot as usize;
                    if self.structure_views.len() <= slot {
                        self.structure_views
                            .resize_with(slot + 1, StructureView::default);
                    }
                    self.structure_views[slot] = StructureView {
                        id: Some(id.clone()),
                        ..default()
                    };
                    self.structure_slots.insert(id.clone(), slot as u32);
                    self.structures_moved = true;
                }
                Change::StructureRemove(slot) => {
                    let view = std::mem::take(&mut self.structure_views[*slot as usize]);
                    if let Some(id) = view.id {
                        self.structure_slots.remove(&id);
                    }
                    self.structures_moved = true;
                }
                Change::StructureCaps { slot, caps } => {
                    self.structure_views[*slot as usize].caps.clone_from(caps);
                }
                Change::MaterialSource(source) => self.material_sources.push(source.clone()),
                Change::Screen(view) => self.screens.push((**view).clone()),
                Change::Screens(items) => self.screen_set = Some(items.clone()),
                Change::Plates { id, texts } => {
                    self.plates.insert(id.clone(), texts.clone());
                }
                Change::Measurements(json) => self.measurements = Some(json.clone()),
                Change::Sandbox(json) => self.sandbox.push(json.clone()),
                Change::Failure(failure) => self.failure = Some(failure.clone()),
                // A grant waits until a renderer step takes it. Its credit is never dropped.
                Change::Grant(grant) => {
                    self.grant = Some(self.grant.map_or(*grant, |g| g.merge(*grant)));
                }
                // The save requests read these from the frame.
                Change::SaveResult { .. } => {}
            }
        }
        let frame = &self.frame;
        for row in &frame.structures {
            let view = &mut self.structure_views[row.slot as usize];
            let range = |r: [u32; 2]| r[0] as usize..r[1] as usize;
            view.flags = row.flags;
            view.lamp = row.lamp;
            view.display_map = row.display_map;
            view.poses.clear();
            view.poses
                .extend_from_slice(&frame.structure_poses[range(row.poses)]);
            view.hidden.clear();
            view.hidden
                .extend_from_slice(&frame.hidden_nodes[range(row.hidden)]);
            view.materials.clear();
            view.materials
                .extend_from_slice(&frame.node_materials[range(row.materials)]);
            view.colors.clear();
            view.colors
                .extend_from_slice(&frame.node_colors[range(row.colors)]);
            self.changed_structures.push(row.slot);
        }
    }
    /// The structure slot of world object `id`.
    pub fn structure_slot(&self, id: &GeneratedId) -> Option<u32> {
        self.structure_slots.get(id).copied()
    }
    pub fn condition(&self, id: ConditionId) -> &ConditionState {
        &self.conditions[&id]
    }
    /// Indexes the particle groups of a new frame.
    pub fn index_particles(&mut self) {
        self.particle_groups.clear();
        for (index, g) in self.frame.particle_groups.iter().enumerate() {
            let key = ParticleGroupKey {
                kind: g.kind,
                a: g.a,
                b: g.b,
                c: g.c,
                source: g.source,
            };
            self.particle_groups.insert(key, index);
        }
    }
    /// The group row and its live particles.
    pub fn particle_group(
        &self,
        key: ParticleGroupKey,
    ) -> Option<(ParticleGroupRow, &[ParticleRow])> {
        let row = self.frame.particle_groups[*self.particle_groups.get(&key)?];
        let start = row.start as usize;
        Some((
            row,
            &self.frame.particles[start..start + row.count as usize],
        ))
    }
    /// Previous and current rows of `key`.
    pub fn pose(&self, key: PoseKey) -> Option<(PoseRow, PoseRow)> {
        let slot = self.poses.get(&key)?.0.0 as usize;
        Some((self.frame.previous[slot], self.frame.current[slot]))
    }
    /// The pose of `key` between the last two steps.
    pub fn interpolated(&self, key: PoseKey) -> Option<VehiclePose> {
        self.pose(key).map(|(previous, current)| {
            super::interpolate(previous.pose(), current.pose(), self.frame.state.alpha)
        })
    }
    pub fn item(&self, item: ItemId) -> Option<ItemRow> {
        let slot = self.poses.get(&PoseKey::Item(item))?.0.0 as usize;
        Some(self.frame.items[slot])
    }
    /// Items with their spawn data.
    pub fn items(&self) -> impl Iterator<Item = (ItemId, SpawnData)> + '_ {
        self.poses.iter().filter_map(|(key, (_, data))| match key {
            PoseKey::Item(id) => Some((*id, *data)),
            PoseKey::Body(_) => None,
        })
    }
    /// Food prototype index and bite of `item`.
    pub fn food(&self, item: ItemId) -> Option<(usize, u8)> {
        match self.poses.get(&PoseKey::Item(item))?.1 {
            SpawnData::Food { prototype } => {
                Some((prototype as usize, self.item(item)?.bite as u8))
            }
            _ => None,
        }
    }
    /// Spawned item prototype bodies in spawn order.
    pub fn spawned(&self) -> &[SpawnedItem] {
        &self.spawned
    }
    /// Skin joint nodes of the player body, in `SimFrame::joints` order.
    pub fn rig(&self) -> &[NodeId] {
        &self.rig
    }
}
