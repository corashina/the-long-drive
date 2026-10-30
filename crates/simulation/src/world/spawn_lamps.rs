//! Source flashlight refresh and point/spot recipes belong to each initialized instance.
use super::spawn_contents::SpawnEvaluation;
use super::spawn_rules::*;
use super::{NodeAppearance, WorldLight};
use crate::reference::{ReferenceError, SourceId};
fn activity(node: SourceId, active: bool) -> NodeAppearance {
    NodeAppearance {
        primary: None,
        node,
        active: Some(active),
        material: None,
        color: None,
        color_material: None,
        rotation: None,
        scale: None,
    }
}
/// Source flashlightscript.Refresh lamp node fields, in `lamp_activity` order.
const LAMP_NODES: [&str; 4] = ["Working", "nWorking", "Lights", "nLights"];
/// Source flashlightscript.Refresh activity of one lamp's `LAMP_NODES`.
fn lamp_activity(broken: bool, lit: bool) -> [bool; 4] {
    [!broken, broken, lit, !broken && !lit]
}
impl SpawnEvaluation<'_> {
    pub(super) fn bake_lamps(&mut self) -> Result<(), ReferenceError> {
        for content in &mut self.output {
            for (parent, source, on) in &self.initialization.lamps {
                if *parent != content.placement.id {
                    continue;
                }
                let f = self.rules.get(self.reference, source)?.fields;
                let powered = lamp_power(
                    self.reference,
                    self.rules,
                    &self.initialization,
                    parent,
                    source,
                )?;
                let reverse = flag(f, "reverseSwitch")?;
                for (field, active) in [("GON", *on ^ reverse), ("GOFF", !(*on ^ reverse))] {
                    if let Some(object) = optional(f, field)? {
                        content.appearance.push(activity(
                            self.rules.root(self.reference, object)?.clone(),
                            active,
                        ));
                    }
                }
                for (index, lamp) in array(f, "Lamps")?.iter().enumerate() {
                    let lamp = fields(lamp)?;
                    let broken = flag(lamp, "broken")?
                        || self.initialization.lamp_broken(parent, source, index);
                    let lit = *on && powered && !broken;
                    for (field, active) in LAMP_NODES.into_iter().zip(lamp_activity(broken, lit)) {
                        for object in array(lamp, field)? {
                            content.appearance.push(activity(
                                self.rules
                                    .root(self.reference, value_link(object)?)?
                                    .clone(),
                                active,
                            ));
                        }
                    }
                    let name = if lit { "mON" } else { "mOFF" };
                    let material = optional(lamp, name)?.or(optional(f, name)?);
                    for renderer in array(lamp, "R")? {
                        content.appearance.push(NodeAppearance {
                            primary: None,
                            node: self
                                .rules
                                .root(self.reference, value_link(renderer)?)?
                                .clone(),
                            active: None,
                            material: material.cloned(),
                            color: None,
                            color_material: None,
                            rotation: None,
                            scale: None,
                        });
                    }
                }
            }
        }
        for index in 0..self.output.len() {
            self.output[index].lights.clear();
            let parent = self.output[index].placement.clone();
            let Some(prototype) = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == parent.prototype)
            else {
                continue;
            };
            for (node_index, node) in prototype.nodes.iter().enumerate() {
                let Some(source) = self
                    .rules
                    .component(self.reference, &node.source, "Light")?
                else {
                    continue;
                };
                let mut record = super::source_lights::light_record(self.reference, source.source)?;
                let mut pose = self.node_pose(&parent, &node.source)?;
                let mut enabled = record.enabled
                    && super::spawn_appearance::node_active(
                        prototype,
                        node_index,
                        &self.output[index].appearance,
                    );
                for (owner, controller, on) in &self.initialization.lamps {
                    if *owner != parent.id {
                        continue;
                    }
                    let f = self.rules.get(self.reference, controller)?.fields;
                    if optional(f, "SingleLight")? != Some(source.source) {
                        continue;
                    }
                    let lamps = array(f, "Lamps")?;
                    let mut count = 0;
                    let mut working = [0.; 3];
                    let mut broken = [0.; 3];
                    for (lamp_index, lamp) in lamps.iter().enumerate() {
                        let lamp = fields(lamp)?;
                        let first = array(lamp, "Working")?.first().ok_or_else(|| {
                            ReferenceError("Single lamp has no working position".into())
                        })?;
                        let position = self
                            .node_pose(
                                &parent,
                                self.rules.root(self.reference, value_link(first)?)?,
                            )?
                            .position;
                        let target = if flag(lamp, "broken")?
                            || self
                                .initialization
                                .lamp_broken(owner, controller, lamp_index)
                        {
                            &mut broken
                        } else {
                            count += 1;
                            &mut working
                        };
                        for i in 0..3 {
                            target[i] += position[i];
                        }
                    }
                    if count > 0 {
                        pose.position = working.map(|v| v / count as f64);
                    } else if !lamps.is_empty() {
                        pose.position = broken.map(|v| v / lamps.len() as f64);
                    }
                    let powered = lamp_power(
                        self.reference,
                        self.rules,
                        &self.initialization,
                        owner,
                        controller,
                    )?;
                    let intensity = if *on && powered && count > 0 {
                        let t = if lamps.len() == 1 {
                            0.
                        } else {
                            ((count as f32 - 1.) / (lamps.len() as f32 - 1.)).clamp(0., 1.)
                        };
                        let low = number(f, "singleLightMinIntensity")?;
                        low + (number(f, "singleLightMaxIntensity")? - low) * t
                    } else {
                        number(f, "singleLightMinMinIntensity")?
                    };
                    record.startup_intensity = intensity;
                    enabled = intensity != 0.;
                }
                for (owner, component) in &self.initialization.initialized {
                    if owner != &parent.id {
                        continue;
                    }
                    let electronics = self.rules.get(self.reference, component)?;
                    if electronics.class_name != "electronicsscript" {
                        continue;
                    }
                    for group in array(electronics.fields, "lights")? {
                        let group = fields(group)?;
                        let lights = array(group, "lights")?;
                        if !lights
                            .iter()
                            .any(|v| value_link(v).ok() == Some(source.source))
                        {
                            continue;
                        }
                        let first = lights.first().ok_or_else(|| {
                            ReferenceError("Source electronics light group empty".into())
                        })?;
                        let original =
                            super::source_lights::light_record(self.reference, value_link(first)?)?;
                        record.startup_intensity =
                            original.startup_intensity * electronics_factor(electronics.fields)?;
                    }
                }
                let mut id = parent.id.clone();
                id.children.push((source.source.clone(), 0));
                self.output[index].lights.push(WorldLight {
                    id,
                    pole: parent.pose,
                    pose,
                    range: record.range_meters,
                    intensity: record.startup_intensity,
                    record,
                    biome: None,
                    prototype: parent.prototype.clone(),
                    scale: parent.scale,
                    enabled,
                    node: Some(node_index as u32),
                });
            }
        }
        Ok(())
    }
}

pub fn electronics_factor(f: Fields<'_>) -> Result<f32, ReferenceError> {
    let maximum = number(f, "overRideMaxJamValue")?;
    let maximum = if maximum > 0. { maximum } else { 0.3 };
    Ok(((maximum - number(f, "jamValue")?) / maximum).clamp(0., 1.))
}
fn lamp_power(
    reference: &crate::reference::WorldReference,
    rules: &SourceRules,
    initialization: &super::spawn_state::SpawnInitialization,
    parent: &super::GeneratedId,
    lamp: &SourceId,
) -> Result<bool, ReferenceError> {
    let mut source = optional(rules.get(reference, lamp)?.fields, "electronics")?;
    for (owner, component) in &initialization.initialized {
        if owner != parent {
            continue;
        }
        let e = rules.get(reference, component)?;
        if e.class_name == "electronicsscript"
            && array(e.fields, "flashlights")?
                .iter()
                .any(|v| value_link(v).ok() == Some(lamp))
        {
            source = Some(component);
        }
    }
    match source {
        Some(source) => {
            let f = rules.get(reference, source)?.fields;
            Ok(number(f, "maxOutput")? * electronics_factor(f)? > 0.2)
        }
        None => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::spawn_state::SpawnInitialization;
    use crate::world::{GeneratedId, WorldLayer};

    #[test]
    fn a_broken_lamp_stays_dark_and_its_neighbor_stays_lit() {
        let source = SourceId {
            file: "sharedassets1.assets".into(),
            path_id: 1,
        };
        let parent = GeneratedId {
            layer: WorldLayer::Building,
            generator: source.clone(),
            unit: [0; 2],
            ordinal: 0,
            children: Vec::new(),
        };
        let mut init = SpawnInitialization::default();
        init.lamps.push((parent.clone(), source.clone(), true));
        // A break trigger of lamp 1. A second break changes nothing.
        assert!(init.break_lamps(&parent, &source, 1 << 1));
        assert!(!init.break_lamps(&parent, &source, 1 << 1));
        let activity = |index| {
            let broken = init.lamp_broken(&parent, &source, index);
            lamp_activity(broken, !broken)
        };
        // Working, nWorking, Lights, nLights.
        assert_eq!(activity(0), [true, false, true, false]);
        assert_eq!(activity(1), [false, true, false, false]);
        // Switched off: lamp 0 shows nLights, lamp 1 stays dark.
        assert_eq!(lamp_activity(false, false), [true, false, false, true]);
        assert_eq!(init.for_parent(&parent).broken_lamps, init.broken_lamps);
    }
}
