//! Atomic nested spawn evaluation. No live random state changes until admission succeeds.
use super::spawn_rules::*;
use super::{AbsolutePose, GeneratedId, Placement, SpawnContext, UnityRandom, WorldBounds};
use crate::reference::{ReferenceError, SourceId, Transform, WorldReference};

#[derive(Clone)]
pub struct SpawnedContent {
    pub inputs: super::spawn_state::SpawnInitialization,
    pub placement: Placement,
    pub appearance: Vec<super::NodeAppearance>,
    /// Road pole cable of a road building. Stream input; `bake_cables` copies it to `appearance`.
    pub cable: Vec<super::NodeAppearance>,
    pub lights: Vec<super::WorldLight>,
    pub fluid: Option<crate::fluids::FluidContainer>,
    pub tanks: Vec<(SourceId, crate::fluids::FluidContainer)>,
    pub buried: bool,
    pub distance_offset: [f32; 3],
    /// RandomSpawned ammoscript count; None keeps the prefab count.
    pub ammo: Option<f32>,
}
impl SpawnedContent {
    /// Switch state of a flashlight that is its own item.
    pub fn portable_lamp(&self) -> Option<bool> {
        self.inputs
            .lamps
            .iter()
            .find(|(parent, _, _)| parent == &self.placement.id)
            .map(|(_, _, on)| *on)
    }
}
pub struct SpawnEvaluation<'a> {
    pub reference: &'a WorldReference,
    pub rules: &'a SourceRules,
    pub random: &'a mut UnityRandom,
    pub context: SpawnContext,
    pub painting_pools: &'a mut Vec<super::spawn_state::PaintingPool>,
    pub output: Vec<SpawnedContent>,
    pub limit: usize,
    depth: usize,
    pub(super) staging: usize,
    admitted: usize,
    pub(super) initialization: super::spawn_state::SpawnInitialization,
    pub(super) deferred: Vec<(Placement, SourceId)>,
    pub(super) starts: Vec<(Placement, Option<super::spawn_starts::ConditionOverride>)>,
    /// Test hook: selected catalog items without a prototype, by exclusion reason.
    #[cfg(test)]
    pub(super) drops: std::collections::HashMap<String, u32>,
}
impl<'a> SpawnEvaluation<'a> {
    pub fn new(
        reference: &'a WorldReference,
        rules: &'a SourceRules,
        random: &'a mut UnityRandom,
        context: SpawnContext,
        painting_pools: &'a mut Vec<super::spawn_state::PaintingPool>,
        limit: usize,
    ) -> Self {
        Self {
            reference,
            rules,
            random,
            context,
            painting_pools,
            output: Vec::new(),
            limit,
            depth: 0,
            staging: 0,
            admitted: 0,
            initialization: super::spawn_state::SpawnInitialization::default(),
            deferred: Vec::new(),
            starts: Vec::new(),
            #[cfg(test)]
            drops: std::collections::HashMap::new(),
        }
    }
    /// Test hook: counts a selected catalog item that has no prototype.
    pub(super) fn note_selection(&mut self, item: usize) {
        #[cfg(test)]
        if let Some(item) = self.reference.catalog.items.get(item)
            && item.prototype.is_none()
        {
            *self.drops.entry(item.exclusion.clone()).or_default() += 1;
        }
        #[cfg(not(test))]
        let _ = item;
    }
    pub(super) fn admit_initialization(
        &mut self,
        parent: &Placement,
    ) -> Result<(), ReferenceError> {
        let bytes = self.rules.initialization_bound(
            self.reference,
            &parent.prototype,
            super::contents::identity_heap(&parent.id),
            0,
        )?;
        let bytes =
            bytes + std::mem::size_of::<Placement>() + super::contents::placement_heap(parent);
        if self.admitted.saturating_add(bytes) > self.staging {
            return Err(ReferenceError(
                "Source initialization exceeds staging admission".into(),
            ));
        }
        self.admitted += bytes;
        Ok(())
    }
    pub(super) fn check_staging(&self, available: usize) -> Result<(), ReferenceError> {
        use std::mem::size_of;
        let bytes = self.output.capacity() * size_of::<SpawnedContent>()
            + self
                .output
                .iter()
                .map(|c| {
                    c.inputs.retained_bytes()
                        + super::contents::placement_heap(&c.placement)
                        + c.tanks.capacity()
                            * size_of::<(SourceId, crate::fluids::FluidContainer)>()
                        + c.tanks
                            .iter()
                            .map(|(s, _)| s.file.capacity())
                            .sum::<usize>()
                        + c.lights.capacity() * size_of::<super::WorldLight>()
                        + c.lights
                            .iter()
                            .map(super::WorldLight::heap_bytes)
                            .sum::<usize>()
                        + super::contents::appearance_bytes(&c.appearance)
                        + super::contents::appearance_bytes(&c.cable)
                })
                .sum::<usize>()
            + self.starts.capacity()
                * size_of::<(Placement, Option<super::spawn_starts::ConditionOverride>)>()
            + self
                .starts
                .iter()
                .map(|(p, _)| super::contents::placement_heap(p))
                .sum::<usize>()
            + self.deferred.capacity() * size_of::<(Placement, SourceId)>()
            + self
                .deferred
                .iter()
                .map(|(p, s)| super::contents::placement_heap(p) + s.file.capacity())
                .sum::<usize>()
            + self.initialization.retained_bytes() * 2;
        if bytes > available {
            return Err(ReferenceError(
                "Source atomic spawn exceeds admitted staging".into(),
            ));
        }
        Ok(())
    }
    pub fn spawn(
        &mut self,
        parent: &Placement,
        caller: &SourceId,
        chance: Option<f32>,
    ) -> Result<(), ReferenceError> {
        if self.depth >= 64 {
            return Err(ReferenceError(
                "Source nested spawn recursion exceeds admission depth".into(),
            ));
        }
        let f = &self.rules.get(self.reference, caller)?.fields;
        let holiday = integer(f, "holiday")?;
        // The checked wall-clock date is independent of the runtime solar clock.
        let month = self.context.date[1];
        let day = self.context.date[2];
        let gate = match holiday {
            0 => true,
            1 => (month == 10 && day >= 15) || (month == 11 && day <= 15),
            2 => (month == 12 && day >= 15) || (month == 1 && day <= 15),
            _ => false,
        };
        if !gate || self.random.range(0.001, 1.) > chance.unwrap_or(number(f, "chanceToSpawn")?) {
            return Ok(());
        }
        let items = array(f, "items")?;
        let mut total = 0i32;
        for row in items {
            let n = integer(fields(row)?, "chance")?;
            total += if n == 0 { 1 } else { n };
        }
        let draw = self.random.range_int(0, total + 1);
        let mut cumulative = 0;
        let mut selected = None;
        for (i, row) in items.iter().enumerate() {
            let row = fields(row)?;
            let n = integer(row, "chance")?;
            cumulative += if n == 0 { 1 } else { n };
            if draw <= cumulative
                && !(self.context.peaceful_enemies == 2 && text(row, "prefab")? == "humanmonster")
            {
                selected = Some(i);
                break;
            }
        }
        let index = selected.unwrap_or(0);
        let Some(row) = items.get(index) else {
            return Ok(());
        };
        let row = fields(row)?;
        let caller_root = self.rules.root(self.reference, caller)?;
        let mut pose = self.node_pose(parent, caller_root)?;
        if selected.is_some() {
            let places = array(row, "places")?;
            let mut total = 0;
            for p in places {
                let n = integer(fields(p)?, "chance")?;
                total += if n == 0 { 1 } else { n };
            }
            let draw = self.random.range_int(0, total + 1);
            let mut cumulative = 0;
            for p in places {
                let p = fields(p)?;
                let n = integer(p, "chance")?;
                cumulative += if n == 0 { 1 } else { n };
                if draw <= cumulative {
                    if let Some(t) = optional(p, "T")? {
                        pose = self.node_pose(parent, t)?;
                    }
                    break;
                }
            }
        }
        // A catalog item without a prototype runs its start rules but does not spawn,
        // such as an other_vehicle car (Car06P).
        let mut spawns = true;
        let target = if let Some(target) = optional(row, "G")? {
            Some(target)
        } else if selected.is_some() {
            let reference = self.reference;
            let item = super::spawn_selection::select_named(
                reference,
                text(row, "prefab")?,
                flag(row, "isitem")?,
                self.random,
                self.context,
            )?;
            if let Some(item) = item {
                self.note_selection(item);
            }
            item.and_then(|i| reference.catalog.items.get(i)).map(|i| {
                spawns = i.prototype.is_some();
                &i.source
            })
        } else {
            None
        };
        let Some(target) = target else { return Ok(()) };
        if flag(row, "randomRot")? {
            pose.rotation =
                super::placement::multiply(yaw(-self.random.range(0., 360.)), pose.rotation);
        }
        let root = self.rules.root(self.reference, target)?.clone();
        let mut id = parent.id.clone();
        id.children.push((caller.clone(), index as u32));
        let child = self.placement(root.clone(), id, pose)?;
        let condition = if flag(row, "disableRandom")? {
            None
        } else {
            Some(super::spawn_starts::ConditionOverride::Explicit([
                integer(row, "ir1")?,
                integer(row, "ir2")?,
            ]))
        };
        self.queue_start(child.clone(), condition);
        self.depth += 1;
        if let Some(spawner) =
            self.rules
                .component(self.reference, target, "newRandomStuffSpawnScript")?
        {
            self.spawn(&child, &spawner.source, None)?;
        } else {
            let mut nested = Vec::new();
            self.rules.children(
                self.reference,
                &root,
                "newRandomStuffSpawnScript",
                false,
                &mut nested,
            )?;
            for spawner in nested {
                self.spawn(&child, spawner, None)?;
            }
            let actual = if let Some(unparent) =
                self.rules
                    .component(self.reference, target, "childunparent")?
            {
                link(&unparent.fields, "g")?
            } else {
                target
            };
            if let Some(save) = self
                .rules
                .component(self.reference, actual, "tosaveitemscript")?
            {
                let mut fluid = None;
                let mut tanks = Vec::new();
                let mut ammo = None;
                if !flag(row, "disableRandom")? {
                    (fluid, tanks, ammo) = self.random_spawned(save, &row)?;
                }
                self.start_save(&child, save, 0)?;
                if spawns
                    && self
                        .reference
                        .prototypes
                        .iter()
                        .any(|p| p.source == child.prototype)
                {
                    self.push(SpawnedContent {
                        inputs: super::spawn_state::SpawnInitialization::default(),
                        placement: child.clone(),
                        appearance: Vec::new(),
                        cable: Vec::new(),
                        lights: Vec::new(),
                        fluid,
                        tanks,
                        buried: flag(save.fields, "buried")?,
                        distance_offset: self.distance_offset(&child, save)?,
                        ammo,
                    })?;
                }
            }
        }
        self.depth -= 1;
        Ok(())
    }
    pub(super) fn distance_offset(
        &self,
        parent: &Placement,
        save: SourceRecord<'_>,
    ) -> Result<[f32; 3], ReferenceError> {
        let node = optional(save.fields, "useThisForDistance")?
            .unwrap_or(self.rules.root(self.reference, save.source)?);
        let world = self.node_pose(parent, node)?;
        let delta = std::array::from_fn(|i| (world.position[i] - parent.pose.position[i]) as f32);
        let q = parent.pose.rotation;
        Ok(super::placement::rotate([-q[0], -q[1], -q[2], q[3]], delta))
    }
    pub(super) fn placement(
        &mut self,
        root: SourceId,
        id: GeneratedId,
        pose: AbsolutePose,
    ) -> Result<Placement, ReferenceError> {
        let scale = self.rules.node(self.reference, &root)?.pose.scale;
        let radius = self.rules.radius(&root).unwrap_or(0.);
        let placement = Placement {
            id,
            prototype: root,
            pose,
            scale,
            bounds: WorldBounds {
                min: pose.position.map(|v| v - radius),
                max: pose.position.map(|v| v + radius),
            },
            object_seed: 0,
        };
        self.admit_initialization(&placement)?;
        Ok(placement)
    }
    pub fn node_pose(
        &self,
        parent: &Placement,
        node: &SourceId,
    ) -> Result<AbsolutePose, ReferenceError> {
        let mut pose = Transform {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        };
        let mut current = node;
        for _ in 0..256 {
            if *current == parent.prototype {
                return Ok(super::placement::place_pose(
                    parent.pose,
                    parent.scale,
                    pose,
                ));
            }
            let node = self.rules.node(self.reference, current)?;
            let t = node.pose.translation;
            let s = node.pose.scale;
            let q = node.pose.rotation;
            let p =
                super::placement::rotate(q, std::array::from_fn(|i| pose.translation[i] * s[i]));
            pose.translation = std::array::from_fn(|i| p[i] + t[i]);
            pose.rotation = super::placement::multiply(q, pose.rotation);
            pose.scale = std::array::from_fn(|i| pose.scale[i] * s[i]);
            let Some(next) = node.parent else {
                return Err(ReferenceError(
                    "Source spawn transform is outside its parent".into(),
                ));
            };
            current = next;
        }
        Err(ReferenceError("Source spawn transform cycle".into()))
    }
}
pub fn yaw(degrees: f32) -> [f32; 4] {
    let a = degrees.to_radians() * 0.5;
    [0., a.sin(), 0., a.cos()]
}
