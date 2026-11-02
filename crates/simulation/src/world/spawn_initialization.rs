//! Synchronous tosaveitemscript initialization, including draws for excluded targets.
use super::Placement;
use super::spawn_contents::{SpawnEvaluation, SpawnedContent};
use super::spawn_rules::*;
use crate::fluids::FluidContainer;
use crate::reference::ReferenceError;
impl SpawnEvaluation<'_> {
    pub(super) fn random_spawned(
        &mut self,
        save: SourceRecord<'_>,
        item: &Fields,
    ) -> Result<
        (
            Option<FluidContainer>,
            Vec<(crate::reference::SourceId, FluidContainer)>,
            Option<f32>,
        ),
        ReferenceError,
    > {
        let f = &save.fields;
        let mut count = None;
        if let Some(ammo) = optional(f, "ammo")? {
            if self
                .rules
                .component(self.reference, &save.source, "sprayscript")?
                .is_none()
            {
                let a = &self.rules.get(self.reference, ammo)?.fields;
                let low = number(item, "fr1")?;
                let high = number(item, "fr2")?;
                let n = self.random.range_int(low as i32, high as i32);
                // SetIntAmmo writes ammo; GetAmmo selects its int or float field.
                let float = flag(a, "floatOrInt")?;
                let mut actual = if float { number(a, "fammo")? } else { n as f32 };
                if actual > number(a, "max")? || low == high {
                    let v = self
                        .random
                        .range(number(a, "ammoRmin")?, number(a, "ammoRmax")?);
                    actual = if float { v } else { v.trunc() };
                }
                count = Some(actual);
            }
        }
        let mut primary = None;
        let mut tanks = Vec::new();
        if let Some(first) = array(f, "tanks")?.first() {
            if let Ok(id) = value_link(first) {
                let tank = self.rules.get(self.reference, id)?;
                primary = Some(self.random_fluid(&tank.fields, Some(item), None)?);
                tanks.push((id.clone(), primary.as_ref().unwrap().clone()));
                for other in array(&tank.fields, "otherTanks")? {
                    let tank = self.rules.get(self.reference, value_link(other)?)?;
                    let fluid = self.random_fluid(&tank.fields, None, None)?;
                    tanks.push((tank.source.clone(), fluid));
                }
            }
        }
        for other in array(f, "randomizetanks")? {
            let tank = self.rules.get(self.reference, value_link(other)?)?;
            let fluid = self.random_fluid(&tank.fields, None, None)?;
            tanks.push((tank.source.clone(), fluid));
        }
        if let Some(food) = optional(f, "food")? {
            let f = &self.rules.get(self.reference, food)?.fields;
            self.random.range(0., 1.);
            self.random
                .range_int(0, array(f, "parts")?.len() as i32 - 2);
        }
        Ok((primary, tanks, count))
    }
    pub(super) fn random_default(
        &mut self,
        save: SourceRecord<'_>,
    ) -> Result<
        (
            Option<FluidContainer>,
            Vec<(crate::reference::SourceId, FluidContainer)>,
        ),
        ReferenceError,
    > {
        let f = &save.fields;
        let mut primary = None;
        let mut tanks = Vec::new();
        if let Some(first) = array(f, "tanks")?.first() {
            if let Ok(id) = value_link(first) {
                let tank = self.rules.get(self.reference, id)?;
                let empty = optional(f, "engine")?.is_some()
                    || self
                        .rules
                        .component(self.reference, &save.source, "coolantTankscript")?
                        .is_some();
                primary = Some(self.random_fluid(&tank.fields, None, Some(empty))?);
                tanks.push((id.clone(), primary.as_ref().unwrap().clone()));
                for other in array(&tank.fields, "otherTanks")? {
                    let tank = self.rules.get(self.reference, value_link(other)?)?;
                    let fluid = self.random_fluid(&tank.fields, None, None)?;
                    tanks.push((tank.source.clone(), fluid));
                }
            }
        }
        for other in array(f, "randomizetanks")? {
            let tank = self.rules.get(self.reference, value_link(other)?)?;
            let fluid = self.random_fluid(&tank.fields, None, None)?;
            tanks.push((tank.source.clone(), fluid));
        }
        Ok((primary, tanks))
    }
    fn random_fluid(
        &mut self,
        f: &Fields,
        item: Option<&Fields>,
        default_empty: Option<bool>,
    ) -> Result<FluidContainer, ReferenceError> {
        let mut low = number(f, "minFluid")?;
        let mut high = number(f, "maxFluid")?;
        let mut empty = number(f, "emptyChance")?;
        let mut mix = number(f, "mixChance")?;
        let mut no_mix = flag(f, "noMixedFluids")?;
        let mut fluids = array(f, "randomFluids")?;
        if let Some(item) = item {
            if !flag(f, "alwaysDefaultFluid")? {
                if flag(item, "overrideFluidAmount")? {
                    low = number(item, "minFluid")?;
                    high = number(item, "maxFluid")?;
                    if low > 0. {
                        empty = -1.;
                    }
                }
                if flag(item, "overrideFluidType")? {
                    fluids = array(item, "fluids")?;
                    no_mix = flag(item, "noMixedFluids")?;
                }
                if flag(item, "overrideMixChance")? {
                    mix = number(item, "mixChance")?;
                }
            }
        }
        if let Some(default_empty) = default_empty {
            empty = if default_empty { 1. } else { 0. };
            mix = 0.;
        }
        let mut result = FluidContainer::new(number(fields(value(f, "F")?)?, "maxC")?, &[]);
        let mut total = 0.;
        for fluid in fluids {
            total += number(fields(fluid)?, "chance")?;
        }
        if self.random.range(0., 1.) > empty {
            if no_mix || fluids.len() == 1 || self.random.range(0., 1.) >= mix {
                let selected = self.random.range(0., total);
                let mut cumulative = 0.;
                for fluid in fluids {
                    let row = fields(fluid)?;
                    cumulative += number(row, "chance")?;
                    if selected < cumulative {
                        let amount = self.random.range(low, high);
                        change_fluid(&mut result, integer(row, "type")?, amount)?;
                        break;
                    }
                }
            } else {
                let mut remaining = self.random.range(low, high);
                for fluid in fluids {
                    let row = fields(fluid)?;
                    if self.random.range(0., 1.) < number(row, "chance")? {
                        let amount = self.random.range(low, remaining);
                        remaining -= change_fluid(&mut result, integer(row, "type")?, amount)?;
                        if remaining <= 0. {
                            break;
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    pub(super) fn start_save(
        &mut self,
        parent: &Placement,
        save: SourceRecord<'_>,
        depth: usize,
    ) -> Result<(), ReferenceError> {
        if depth >= 64 {
            return Err(ReferenceError(
                "Source attached item initialization cycle".into(),
            ));
        }
        let f = &save.fields;
        if flag(f, "started")? || !self.mark_started(&parent.id, &save.source) {
            return Ok(());
        }
        for slot in array(f, "partslotscripts")? {
            let Ok(slot) = value_link(slot) else { continue };
            let slot = self.rules.get(self.reference, slot)?;
            if let Some(part) = self
                .initialization
                .slot_parts
                .iter()
                .find(|(p, s, _)| *p == parent.id && s == slot.source)
                .map(|v| &v.2)
                .or(optional(&slot.fields, "part")?)
            {
                let part = self.rules.get(self.reference, part)?;
                if let Some(save) = optional(&part.fields, "tosaveitem")? {
                    self.start_save(parent, self.rules.get(self.reference, save)?, depth + 1)?;
                }
            }
        }
        for scale in array(f, "randomScales")? {
            let f = &self.rules.get(self.reference, value_link(scale)?)?.fields;
            if !flag(f, "loaded")? {
                for scale in array(f, "scales")? {
                    let scale = fields(scale)?;
                    let min = vector(scale, "minScale")?;
                    let max = vector(scale, "maxScale")?;
                    let x = self.random.range(min[0], max[0]);
                    let size = if flag(scale, "one")? {
                        [x; 3]
                    } else {
                        [
                            x,
                            self.random.range(min[1], max[1]),
                            self.random.range(min[2], max[2]),
                        ]
                    };
                    for object in array(scale, "objs")? {
                        let node = self.rules.root(self.reference, value_link(object)?)?;
                        if let Some(state) = self
                            .initialization
                            .scales
                            .iter_mut()
                            .find(|(p, s, _)| p == &parent.id && s == node)
                        {
                            state.2 = size;
                        } else {
                            self.initialization.scales.push((
                                parent.id.clone(),
                                node.clone(),
                                size,
                            ));
                        }
                    }
                }
            }
        }
        if let Some(painting) = optional(f, "painting")? {
            self.painting(parent, self.rules.get(self.reference, painting)?)?;
        }
        if let Some(box_id) = optional(f, "spawnBox")? {
            let b = self.rules.get(self.reference, box_id)?;
            let f = &b.fields;
            if !flag(f, "disableSpawn")? {
                let selected = super::spawn_selection::select_pool(
                    self.reference,
                    array(f, "spawn")?,
                    self.random,
                    self.context,
                )?;
                if flag(f, "spawnBeforeOpen")? && !flag(f, "spawned")? {
                    if let Some(item) = selected {
                        self.note_selection(item);
                    }
                    if let Some(item) = selected.and_then(|i| self.reference.catalog.items.get(i)) {
                        let root = self.rules.root(self.reference, &item.source)?.clone();
                        let point = optional(f, "spawnPos")?
                            .unwrap_or(self.rules.root(self.reference, box_id)?);
                        let origin = self.node_pose(parent, point)?;
                        let mut count = 1;
                        let mut i = 0;
                        loop {
                            let mut pose = origin;
                            if i > 0 {
                                let sphere = self.random.on_unit_sphere();
                                let scale = self.rules.node(self.reference, point)?.pose.scale[0];
                                pose.position = std::array::from_fn(|a| {
                                    pose.position[a]
                                        + sphere[a] as f64
                                            * scale as f64
                                            * 0.15
                                            * if a == 2 { -1. } else { 1. }
                                });
                            }
                            if i == 0 {
                                if let Some(mass) =
                                    self.rules.component(self.reference, &root, "massScript")?
                                {
                                    count = super::spawn_selection::box_count(
                                        integer(mass.fields, "maxCountIfSpawnedInBox")?,
                                        self.random,
                                    );
                                }
                            }
                            let mut id = parent.id.clone();
                            id.children.push((box_id.clone(), i as u32));
                            let child = self.placement(root.clone(), id, pose)?;
                            if let Some(save) =
                                self.rules
                                    .component(self.reference, &root, "tosaveitemscript")?
                            {
                                self.queue_start(
                                    child.clone(),
                                    Some(super::spawn_starts::ConditionOverride::Default),
                                );
                                self.start_save(&child, save, depth + 1)?;
                                let (fluid, tanks) = self.random_default(save)?;
                                if self.reference.prototypes.iter().any(|p| p.source == root) {
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
                                        ammo: None,
                                    })?;
                                }
                            }
                            self.random.on_unit_sphere();
                            i += 1;
                            if i >= count {
                                break;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn painting(
        &mut self,
        parent: &Placement,
        painting: SourceRecord<'_>,
    ) -> Result<(), ReferenceError> {
        let f = painting.fields;
        if !self.mark_started(&parent.id, painting.source) {
            return Ok(());
        }
        let mut chosen = integer(f, "index")?;
        if !flag(f, "selected")?
            && let Some(main) = optional(f, "main")?
        {
            let fields = self.rules.get(self.reference, main)?.fields;
            let index =
                if let Some(index) = self.painting_pools.iter().position(|p| p.source == *main) {
                    index
                } else {
                    let available = pool_indices(array(fields, "available")?)?;
                    self.painting_pools.push(super::spawn_state::PaintingPool {
                        source: main.clone(),
                        available,
                    });
                    self.painting_pools.len() - 1
                };
            if self.painting_pools[index].available.is_empty() {
                self.painting_pools[index].available = pool_indices(array(fields, "defindex")?)?;
            }
            let count = self.painting_pools[index].available.len();
            let selected = self.random.range_int(0, count as i32);
            if count == 0 {
                return Err(ReferenceError("Source painting pool is empty".into()));
            }
            chosen = self.painting_pools[index]
                .available
                .remove(selected as usize);
        }
        self.initialization
            .paintings
            .push((parent.id.clone(), painting.source.clone(), chosen));
        Ok(())
    }
    pub(super) fn push(&mut self, value: SpawnedContent) -> Result<(), ReferenceError> {
        if self.output.len() >= self.limit {
            return Err(ReferenceError(
                "Source nested contents exceed object budget".into(),
            ));
        }
        self.output.push(value);
        Ok(())
    }
}
fn change_fluid(
    container: &mut FluidContainer,
    kind: i32,
    amount: f32,
) -> Result<f32, ReferenceError> {
    if !(0..6).contains(&kind) {
        return Err(ReferenceError("Unsupported source fluid type".into()));
    }
    let current = container.mixture().quantities_liters[kind as usize];
    let accepted = amount
        .max(-current)
        .min((container.capacity_liters - container.total()).max(0.));
    container.change_one(accepted, kind as usize);
    Ok(accepted)
}

fn pool_indices(values: &[crate::reference::WorldValue]) -> Result<Vec<i32>, ReferenceError> {
    values
        .iter()
        .map(|v| {
            if let crate::reference::WorldValue::Integer(n) = v {
                Ok(*n as i32)
            } else {
                Err(ReferenceError(
                    "Source painting pool index is not an integer".into(),
                ))
            }
        })
        .collect()
}
