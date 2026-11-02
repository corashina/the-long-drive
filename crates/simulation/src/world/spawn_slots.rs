//! Deferred partslotscript continuation. Source selection precedes supported publication.
use super::spawn_contents::{SpawnEvaluation, SpawnedContent};
use super::spawn_rules::*;
use super::{AbsolutePose, Placement};
use crate::reference::{ReferenceError, SourceId};
impl SpawnEvaluation<'_> {
    pub(super) fn spawn_slot(
        &mut self,
        parent: &Placement,
        source: &SourceId,
    ) -> Result<(), ReferenceError> {
        let f = self.rules.get(self.reference, source)?.fields;
        if let Some(selector) = optional(f, "randomTypeToStart")? {
            self.start_selector(parent, self.rules.get(self.reference, selector)?)?;
            if let Some(part) = self.slot_part(parent, source, f)? {
                let part = self.rules.get(self.reference, part)?;
                let save = link(part.fields, "tosaveitem")?;
                self.start_save(parent, self.rules.get(self.reference, save)?, 0)?;
            }
        }
        if flag(f, "enableSpawn")?
            && self.slot_part(parent, source, f)?.is_none()
            && self.random.range(0., 1.) < number(f, "spawnChance")?
        {
            let mut selected = integer(f, "selectedID")?;
            if self.random.range(0., 1.) < number(f, "randomChance")? {
                let rows = array(f, "randomParts")?;
                let total = rows.iter().try_fold(0., |n, row| {
                    Ok::<_, ReferenceError>(n + number(fields(row)?, "chance")?)
                })?;
                let draw = self.random.range(0., total);
                let mut cumulative = 0.;
                for row in rows {
                    let row = fields(row)?;
                    cumulative += number(row, "chance")?;
                    if draw <= cumulative {
                        selected = integer(row, "id")?;
                        break;
                    }
                }
            } else {
                selected = integer(f, "spawn_def")?;
            }
            let item = self
                .reference
                .catalog
                .items
                .get(selected as usize)
                .ok_or_else(|| {
                    ReferenceError("Source slot item is outside its checked catalog".into())
                })?;
            let root = self.rules.root(self.reference, &item.source)?.clone();
            let original = self.rules.node(self.reference, &root)?.pose;
            let mut id = parent.id.clone();
            id.children.push((source.clone(), 0));
            let mut child = self.placement(
                root.clone(),
                id,
                AbsolutePose {
                    position: original.translation.map(f64::from),
                    rotation: original.rotation,
                },
            )?;
            let part = self.rules.component(self.reference, &root, "partscript")?;
            if let Some(part) = part {
                let save = self
                    .rules
                    .get(self.reference, link(part.fields, "tosaveitem")?)?;
                self.start_save(&child, save, 0)?;
                let slot = optional(f, "T")?.unwrap_or(self.rules.root(self.reference, source)?);
                let mut pose = self.node_pose(parent, slot)?;
                pose.rotation = super::placement::multiply(
                    pose.rotation,
                    super::spawn_building::source_euler(vector(
                        part.fields,
                        "localRotationAfterMount",
                    )?),
                );
                child.pose = pose;
                let radius = self.rules.radius(&root).unwrap_or(0.);
                child.bounds.min = pose.position.map(|v| v - radius);
                child.bounds.max = pose.position.map(|v| v + radius);
                self.queue_start(child.clone(), None);
                if item.prototype.is_some() {
                    self.push(SpawnedContent {
                        inputs: super::spawn_state::SpawnInitialization::default(),
                        placement: child.clone(),
                        appearance: Vec::new(),
                        cable: Vec::new(),
                        lights: Vec::new(),
                        fluid: None,
                        tanks: Vec::new(),
                        buried: flag(save.fields, "buried")?,
                        distance_offset: self.distance_offset(&child, save)?,
                        ammo: None,
                    })?;
                }
            } else {
                self.queue_start(child, None);
            }
        }
        Ok(())
    }
    fn slot_part<'b>(
        &'b self,
        parent: &Placement,
        source: &SourceId,
        f: Fields<'b>,
    ) -> Result<Option<&'b SourceId>, ReferenceError> {
        Ok(self
            .initialization
            .slot_parts
            .iter()
            .find(|(p, s, _)| *p == parent.id && s == source)
            .map(|v| &v.2)
            .or(optional(f, "part")?))
    }
}
