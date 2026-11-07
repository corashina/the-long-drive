//! Source `electronicsscript` on generated contents. The jam dims its `lights` groups and
//! switches the flashlights it powers.
use super::WorldStream;
use super::content_kinds::Bound;
use crate::electronics::{Electronics, Emp};
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, SourceId};
use crate::world::GeneratedId;
use crate::world::spawn_lamps::LIGHT_TURN_OFF;
use crate::world::spawn_rules::*;
use rapier3d::math::Vector;

pub struct ElectronicsBinding {
    parent: GeneratedId,
    component: SourceId,
    /// Collider layer of the component node.
    layer: u32,
    state: Electronics,
    /// Light components of each `lights` group and the group maximum intensity.
    lights: Vec<(SourceId, f32)>,
    /// The component powers flashlights. Their refresh follows the power.
    lamps: bool,
    /// Power and light factor of the last refresh.
    powered: bool,
    factor: f32,
}

impl WorldStream {
    /// Binds each electronicsscript of the generated contents and drops gone ones.
    pub(super) fn sync_electronics_bindings(&mut self) -> Result<(), ReferenceError> {
        let revision = (
            self.request.session,
            self.contents.active_revision() ^ self.contents.roots_revision().rotate_left(32),
        );
        if self.kinds.electronics.scan == Some(revision) {
            return Ok(());
        }
        let contents = &self.contents;
        self.kinds
            .electronics
            .list
            .retain(|b| contents.instances().any(|c| c.placement.id == b.parent));
        let (reference, rules) = (&self.reference, &self.contents.rules);
        for content in self.contents.instances() {
            let parent = &content.placement.id;
            for (owner, component) in &content.inputs.initialized {
                if owner != parent
                    || self
                        .kinds
                        .electronics
                        .list
                        .iter()
                        .any(|b| &b.parent == parent && &b.component == component)
                {
                    continue;
                }
                let record = rules.get(reference, component)?;
                if record.class_name != "electronicsscript" {
                    continue;
                }
                let f = record.fields;
                let mut lights = Vec::new();
                for group in array(f, "lights")? {
                    let group = array(fields(group)?, "lights")?;
                    let Some(first) = group.first() else {
                        return Err(ReferenceError(
                            "Source electronics light group empty".into(),
                        ));
                    };
                    let max =
                        crate::world::source_lights::light_record(reference, value_link(first)?)?
                            .startup_intensity;
                    for light in group {
                        lights.push((value_link(light)?.clone(), max));
                    }
                }
                let mut lamps = !array(f, "flashlights")?.is_empty();
                for (p, lamp, _) in &content.inputs.lamps {
                    lamps |= p == parent
                        && optional(rules.get(reference, lamp)?.fields, "electronics")?
                            == Some(component);
                }
                if lights.is_empty() && !lamps {
                    continue;
                }
                let root = rules.root(reference, component)?;
                let layer = reference
                    .prototypes
                    .find(&content.placement.prototype)
                    .and_then(|p| p.nodes.find(root))
                    .map_or(0, |n| u32::from(n.layer));
                let mut state = Electronics::from_fields(f)?;
                if let Some(jam) = content.inputs.jam(parent, component) {
                    state.jam = jam;
                }
                self.kinds.electronics.list.push(ElectronicsBinding {
                    parent: parent.clone(),
                    component: component.clone(),
                    layer,
                    powered: state.output() > LIGHT_TURN_OFF,
                    factor: state.factor(),
                    state,
                    lights,
                    lamps,
                });
            }
        }
        self.kinds.electronics.scan = Some(revision);
        Ok(())
    }

    /// EMPs jam each bound electronicsscript at its content position. Its lights take the new
    /// output. A power change refreshes its flashlights.
    pub fn expose_electronics(
        &mut self,
        emps: &[Emp],
        items: &ItemStore,
        dt: f32,
    ) -> Result<(), ReferenceError> {
        for index in 0..self.kinds.electronics.list.len() {
            let b = &self.kinds.electronics.list[index];
            let active = self
                .contents
                .active()
                .iter()
                .find(|c| c.content.placement.id == b.parent);
            let at = match active.and_then(|c| c.item).and_then(|i| items.get(i)) {
                Some(item) => Vector::from_array(item.pose.position),
                None => {
                    let Some(content) = self
                        .contents
                        .instances()
                        .find(|c| c.placement.id == b.parent)
                    else {
                        continue;
                    };
                    Vector::from_array(self.origin.local(content.placement.pose.position))
                }
            };
            let b = &mut self.kinds.electronics.list[index];
            let (layer, jam) = (b.layer, b.state.jam);
            b.state.expose(emps, at, layer, dt);
            if b.state.jam == jam && b.state.factor() == b.factor {
                continue;
            }
            let powered = b.state.output() > LIGHT_TURN_OFF;
            if b.lamps && powered != b.powered {
                let (parent, component, jam) = (b.parent.clone(), b.component.clone(), b.state.jam);
                b.powered = powered;
                b.factor = b.state.factor();
                // The bake reads the jam. Flashlights and lights refresh together.
                self.contents
                    .set_jam(&self.reference, &parent, &component, jam)?;
                self.refresh_lamp_outputs();
                continue;
            }
            b.factor = b.state.factor();
            let factor = b.factor;
            let parent = self.kinds.electronics.list[index].parent.clone();
            self.set_component_lights(&parent, index, factor);
        }
        Ok(())
    }

    /// Light intensity of each `lights` group of binding `index`: its maximum times `factor`.
    fn set_component_lights(&mut self, parent: &GeneratedId, index: usize, factor: f32) {
        let lights = &self.kinds.electronics.list[index].lights;
        let Some(content) = self.contents.instance_mut(parent) else {
            return;
        };
        for light in &mut content.lights {
            let Some((_, max)) = light
                .id
                .children
                .last()
                .and_then(|(source, _)| lights.iter().find(|(s, _)| s == source))
            else {
                continue;
            };
            light.intensity = max * factor;
            if let Some(current) = self.lights.iter_mut().find(|l| l.id == light.id) {
                current.intensity = light.intensity;
            }
        }
    }
}

impl Bound for ElectronicsBinding {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        None
    }
}
