//! Source building prelude and underground callers.
use super::Placement;
use super::spawn_contents::{SpawnEvaluation, SpawnedContent, yaw};
use super::spawn_rules::*;
use crate::reference::{ReferenceError, SourceId};
/// Source Halloween window: 15 October through 15 November.
fn halloween(date: [i32; 3]) -> bool {
    (date[1] == 10 && date[2] >= 15) || (date[1] == 11 && date[2] <= 15)
}
impl SpawnEvaluation<'_> {
    pub(super) fn building(
        &mut self,
        parent: &Placement,
        building: &SourceId,
    ) -> Result<Option<f32>, ReferenceError> {
        let f = &self.rules.get(self.reference, building)?.fields;
        // paintingMain sits on the building root, so each building instance has a fresh pool.
        self.painting_pools.clear();
        let low = number(f, "minRandomSpawn")?;
        let high = number(f, "maxRandomSpawn")?;
        let chance = (low != 0. || high != 0.).then(|| self.random.range(low, high));
        let mut underground = Vec::new();
        self.rules.children(
            self.reference,
            &parent.prototype,
            "undergroundSpawnScript",
            true,
            &mut underground,
        )?;
        for caller in underground {
            self.underground(parent, caller)?;
        }
        for save in array(f, "toStart")? {
            if let Ok(save) = value_link(save) {
                self.start_save(parent, self.rules.get(self.reference, save)?, 0)?;
            }
        }
        for painting in array(f, "paintings")? {
            if let Ok(p) = value_link(painting) {
                self.painting(parent, self.rules.get(self.reference, p)?)?;
            }
        }
        if flag(f, "forceRandomFlyer")?
            || (flag(f, "randomFlyerOnHalloween")?
                && halloween(self.context.date)
                && self.random.range(0., 1.) > 0.35)
        {
            let count = self.random.range_int(5, 30);
            let main = SourceId {
                file: "level1".into(),
                path_id: 64919,
            };
            let main = self.rules.get(self.reference, &main)?;
            let target = link(&main.fields, "randomFlyerPrefab")?;
            for i in 0..count {
                let sphere = self.random.on_unit_sphere();
                let radius = self.random.range(
                    number(f, "randomFlyerHorizontalMin")?,
                    number(f, "randomFlyerHorizontalMax")?,
                );
                let position = [
                    parent.pose.position[0] + (sphere[0] * radius) as f64,
                    parent.pose.position[1]
                        + (number(f, "randomFlyerOffsetty")?
                            + sphere[1] * number(f, "randomFlyerVertical")?)
                            as f64,
                    parent.pose.position[2] - (sphere[2] * radius) as f64,
                ];
                let mut id = parent.id.clone();
                id.children.push((building.clone(), i as u32));
                let child = self.placement(
                    self.rules.root(self.reference, target)?.clone(),
                    id,
                    super::AbsolutePose {
                        position,
                        rotation: [0., 0., 0., 1.],
                    },
                )?;
                if let Some(save) =
                    self.rules
                        .component(self.reference, target, "tosaveitemscript")?
                {
                    self.start_save(&child, save, 0)?;
                }
                self.queue_start(child, None);
            }
        }
        Ok(chance)
    }
    pub(super) fn underground(
        &mut self,
        parent: &Placement,
        caller: &SourceId,
    ) -> Result<(), ReferenceError> {
        let f = &self.rules.get(self.reference, caller)?.fields;
        if self.random.range(0., 1.) >= number(f, "chance")? {
            return Ok(());
        }
        let count = self
            .random
            .range_int(integer(f, "minCount")?, integer(f, "maxCount")? + 1);
        let center = self.node_pose(parent, link(f, "center")?)?;
        for i in 0..count {
            let angle = self.random.range(0., 360.);
            let radius = self
                .random
                .range(number(f, "minRadius")?, number(f, "maxRadius")?);
            let height = self
                .random
                .range(number(f, "minOffsetty")?, number(f, "maxOffsetty")?);
            let offset = super::placement::rotate(yaw(-angle), [0., 0., -radius]);
            let pose_position = [
                center.position[0] + offset[0] as f64,
                center.position[1] + height as f64,
                center.position[2] + offset[2] as f64,
            ];
            let selection = super::spawn_selection::select_pool(
                self.reference,
                array(f, "spawn")?,
                self.random,
                self.context,
            )?;
            if let Some(item) = selection {
                self.note_selection(item);
            }
            let Some(item) = selection.and_then(|i| self.reference.catalog.items.get(i)) else {
                continue;
            };
            let euler = self.random.on_unit_sphere().map(|v| v * 360.);
            let pose = super::AbsolutePose {
                position: pose_position,
                rotation: source_euler(euler),
            };
            let mut id = parent.id.clone();
            id.children.push((caller.clone(), i as u32));
            let child = self.placement(
                self.rules.root(self.reference, &item.source)?.clone(),
                id,
                pose,
            )?;
            if let Some(save) =
                self.rules
                    .component(self.reference, &item.source, "tosaveitemscript")?
            {
                self.start_save(&child, save, 0)?;
                let (fluid, tanks) = self.random_default(save)?;
                self.queue_start(
                    child.clone(),
                    Some(super::spawn_starts::ConditionOverride::Default),
                );
                if item.prototype.is_some() {
                    self.push(SpawnedContent {
                        inputs: super::spawn_state::SpawnInitialization::default(),
                        placement: child.clone(),
                        appearance: Vec::new(),
                        cable: Vec::new(),
                        lights: Vec::new(),
                        fluid,
                        tanks,
                        buried: true,
                        distance_offset: self.distance_offset(&child, save)?,
                        ammo: None,
                    })?;
                }
            }
        }
        Ok(())
    }
}
pub fn source_euler(v: [f32; 3]) -> [f32; 4] {
    let v = v.map(|v| v.to_radians() * 0.5);
    let x = [v[0].sin(), 0., 0., v[0].cos()];
    let y = [0., v[1].sin(), 0., v[1].cos()];
    let z = [0., 0., v[2].sin(), v[2].cos()];
    let q = super::placement::multiply(super::placement::multiply(y, x), z);
    [-q[0], -q[1], q[2], q[3]]
}
#[cfg(test)]
mod tests {
    #[test]
    fn halloween_window() {
        assert!(!super::halloween([2026, 10, 14]));
        assert!(super::halloween([2026, 10, 15]));
        assert!(super::halloween([2026, 11, 15]));
        assert!(!super::halloween([2026, 11, 16]));
    }
}
