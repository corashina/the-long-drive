//! Source metal detectors: metalDetectorScript scan, on/off switch and meter needle on
//! generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::interaction::items::ItemStore;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{
    COLLIDER_TAGS, ColliderId, CurveKey, ItemId, ReferenceError, SourceId, TorqueCurve, Transform,
    WorldValue,
};
use crate::simulation::dials::{inverse_lerp, lerp, unity_euler};
use crate::simulation::metal_detector::{DetectorRules, MetalDetector};
use crate::vehicle::{PhysicsWorld, to_rapier};
use crate::world::spawn_rules::*;
use crate::world::unity_random::UnityRandom;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::math::Pose;

/// meterscript `mutatok` row of meterType: mutatoscript without lerpAngle.
pub struct DetectorNeedle {
    /// Prototype node index.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub node: usize,
    pub pose: Transform,
    axis: usize,
    inv: bool,
    rest: [f32; 3],
    value: f32,
    min_angle: f32,
    max_angle: f32,
    min_value: f32,
    max_value: f32,
    offset: f32,
    speed: f32,
}
pub struct DetectorBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    item: ItemId,
    knobs: Vec<ColliderId>,
    rules: DetectorRules,
    detector: MetalDetector,
    /// detectorTransform relative to the item root.
    transform: Pose,
    random: UnityRandom,
    hits: Vec<(f32, u128)>,
    pub needle: DetectorNeedle,
}
impl DetectorBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.knobs.capacity() * std::mem::size_of::<ColliderId>()
            + (self.rules.rescale.keys.capacity() + self.rules.range_curve.keys.capacity())
                * std::mem::size_of::<CurveKey>()
            + self.rules.tags.capacity() * std::mem::size_of::<crate::reference::ColliderTag>()
            + self.hits.capacity() * std::mem::size_of::<(f32, u128)>()
    }
}

/// An unweighted source AnimationCurve.
fn curve<'a>(f: impl FieldAccess<'a>, name: &str) -> Result<TorqueCurve, ReferenceError> {
    let c = fields(value(f, name)?)?;
    let mut keys = Vec::new();
    for k in array(c, "m_Curve")? {
        let k = fields(k)?;
        if integer(k, "weightedMode")? != 0 {
            return Err(ReferenceError(format!("Weighted detector curve {name}")));
        }
        keys.push(CurveKey {
            time: number(k, "time")?,
            value: number(k, "value")?,
            in_tangent: number(k, "inSlope")?,
            out_tangent: number(k, "outSlope")?,
            in_weight: number(k, "inWeight")?,
            out_weight: number(k, "outWeight")?,
            weighted_mode: 0,
        });
    }
    if keys.is_empty() {
        return Err(ReferenceError(format!("Empty detector curve {name}")));
    }
    Ok(TorqueCurve {
        keys,
        pre_wrap_mode: integer(c, "m_PreInfinity")?,
        post_wrap_mode: integer(c, "m_PostInfinity")?,
    })
}

impl WorldStream {
    pub(super) fn sync_detector_bindings(
        &mut self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.detector_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.detector_bindings.len() {
            let b = &mut self.detector_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                b.slot = slot;
                i += 1;
            } else {
                for knob in &b.knobs {
                    interaction.remove_dial_knob(*knob);
                }
                self.detector_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if self.detector_bindings.iter().any(|b| b.item == item) {
                continue;
            }
            let root = &content.content.placement.prototype;
            let Some(script) =
                self.contents
                    .rules
                    .component(&self.reference, root, "metalDetectorScript")?
            else {
                continue;
            };
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let script = script.source.clone();
            let binding = self.bind_detector(items, interaction, physics, slot, item, &script)?;
            self.detector_bindings.push(binding);
        }
        self.detector_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_detector(
        &self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
        script: &SourceId,
    ) -> Result<DetectorBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &self.reference;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == content.content.placement.prototype)
            .ok_or_else(|| ReferenceError("Metal detector prototype missing".into()))?;
        let index = |link: &SourceId| -> Result<usize, ReferenceError> {
            prototype
                .nodes
                .iter()
                .position(|n| &n.source == link)
                .ok_or_else(|| ReferenceError("Metal detector node outside its prototype".into()))
        };
        let f = rules.get(reference, script)?.fields;
        // Source queries ignore triggers; the scan excludes sensors.
        if integer(f, "detectorTriggerInteraction")? != 1 {
            return Err(ReferenceError("Metal detector must ignore triggers".into()));
        }
        let mut tags = Vec::new();
        for t in array(f, "detectTags")? {
            let WorldValue::Text(t) = t else {
                return Err(ReferenceError("Invalid metal detector tag".into()));
            };
            tags.push(
                *COLLIDER_TAGS
                    .iter()
                    .find(|c| format!("{c:?}") == *t)
                    .ok_or_else(|| ReferenceError(format!("Unknown metal detector tag {t}")))?,
            );
        }
        let mask = fields(value(f, "detectorLayerMask")?)?;
        let detector_rules = DetectorRules {
            ray_count: integer(f, "rayCount")?.max(0) as u32,
            scan_seconds: number(f, "scanMinTime")?,
            hits: integer(f, "hitsArraySize")?.max(0) as usize,
            min_range: number(f, "detectorMinRange")?,
            max_range: number(f, "detectorMaxRange")?,
            ray_start_radius: number(f, "rayStartRadius")?,
            ray_end_radius: number(f, "rayEndRadius")?,
            ray_distance: number(f, "rayDistance")?,
            wall_max: number(f, "wallMaxRangeMultiplier")?,
            wall_min: number(f, "wallMinRangeMultiplier")?,
            rescale: curve(f, "detectorReScale")?,
            range_curve: curve(f, "detectorRangeCurve")?,
            tags,
            mask: integer(mask, "m_Bits")? as u32,
            pitch: [number(f, "beepMinPitch")?, number(f, "beepMaxPitch")?],
            pitch_values: [
                number(f, "beepMinPitchValue")?,
                number(f, "beepMaxPitchValue")?,
            ],
            volume: [number(f, "beepMinVolume")?, number(f, "beepMaxVolume")?],
            volume_values: [
                number(f, "beepMinVolumeValue")?,
                number(f, "beepMaxVolumeValue")?,
            ],
            lerp_speed: number(f, "valueLerpSpeed")?,
        };
        let (transform, _) = prototype.root_relative(index(link(f, "detectorTransform")?)?);
        // meterscript row of meterType drives one mutatoscript.
        let kind = text(f, "meterType")?;
        let meter = rules.get(reference, link(f, "meter")?)?.fields;
        let mut row = None;
        for m in array(meter, "mutatok")? {
            let m = fields(m)?;
            if text(m, "tipus")? == kind {
                row = Some(link(m, "mutato")?);
            }
        }
        let n = rules
            .get(
                reference,
                row.ok_or_else(|| ReferenceError("Metal detector meter has no needle".into()))?,
            )?
            .fields;
        if flag(n, "noValueLerp")? || flag(n, "lerpAngle")? {
            return Err(ReferenceError("Unsupported metal detector needle".into()));
        }
        // Source checks X, then Y, then Z; none falls back to X.
        let axis = ["X", "Y", "Z"]
            .iter()
            .map(|a| flag(n, a))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .position(|a| *a)
            .unwrap_or(0);
        let node = link(n, "mutato")?;
        let multiplier = number(n, "lerpSpeedMultiplier")?;
        let needle = DetectorNeedle {
            node: index(node)?,
            pose: rules.node(reference, node)?.pose,
            axis,
            inv: flag(n, "inv")?,
            rest: [number(n, "x")?, number(n, "y")?, number(n, "z")?],
            value: 0.,
            min_angle: number(n, "minAngle")?,
            max_angle: number(n, "maxAngle")?,
            min_value: number(n, "minValue")?,
            max_value: number(n, "maxValue")?,
            offset: number(n, "inputValueOffset")?,
            speed: 8. * if multiplier == 0. { 1. } else { multiplier },
        };
        // The switch is every collider on the turnable onOffButton's own transform.
        let usable = link(f, "onOffButton")?;
        let u = rules.get(reference, usable)?.fields;
        if !flag(u, "turnable")? || integer(u, "maxTurnState")? != 1 {
            return Err(ReferenceError(
                "Metal detector switch is not a two-state usable".into(),
            ));
        }
        let mut on = integer(u, "currentTurnState")? != 0;
        if let Some(saved) = self
            .changes
            .get(ObjectKey::Generated(parent.clone()))
            .and_then(|c| c.snapshot.as_ref())
            .and_then(|s| s.detector)
        {
            on = saved;
        }
        let switch_node = rules.root(reference, usable)?;
        let body = items.get(item).and_then(|i| i.body);
        let mut knobs = Vec::new();
        let colliders = self
            .structures
            .iter()
            .filter(|s| s.placement.id == parent)
            .flat_map(|s| &s.colliders);
        for (source, handle) in colliders {
            let on_switch = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| &prototype.nodes[c.node as usize].source == switch_node);
            if !on_switch || physics.rapier().colliders[*handle].parent() != body {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            interaction.bind_dial_knob(id, Knob::Switch);
            knobs.push(id);
        }
        let hits = Vec::with_capacity(detector_rules.hits.max(16));
        Ok(DetectorBinding {
            parent,
            slot,
            item,
            knobs,
            rules: detector_rules,
            detector: MetalDetector::new(on),
            transform,
            random: UnityRandom::new(item.0 as i32),
            hits,
            needle,
        })
    }
    /// usablescript.Turn on the onOffButton. Returns false when the knob is not a switch.
    pub fn switch_detector(&mut self, knob: ColliderId, items: &mut ItemStore) -> bool {
        let Some(b) = self
            .detector_bindings
            .iter_mut()
            .find(|b| b.knobs.contains(&knob))
        else {
            return false;
        };
        b.detector.turn();
        items.mark_dirty(b.item);
        true
    }
    /// The switch state saved with a detector item.
    pub(super) fn detector_value(&self, item: ItemId) -> Option<bool> {
        self.detector_bindings
            .iter()
            .find(|b| b.item == item)
            .map(|b| b.detector.on)
    }
    /// metalDetectorScript.Update and its mutatoscript needle. Calls `beep` with the item,
    /// detector position, pitch and volume of each detector that is on.
    pub fn advance_detectors(
        &mut self,
        dt: f32,
        items: &ItemStore,
        physics: &PhysicsWorld,
        mut beep: impl FnMut(ItemId, [f32; 3], f32, f32),
    ) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        for b in &mut self.detector_bindings {
            let Some(item) = items.get(b.item) else {
                continue;
            };
            let transform = to_rapier(item.pose) * b.transform;
            if let Some((pitch, volume)) = b.detector.advance(
                &b.rules,
                dt,
                transform,
                item.body,
                physics,
                &mut b.random,
                &mut b.hits,
            ) {
                beep(b.item, transform.translation.to_array(), pitch, volume);
            }
            let n = &mut b.needle;
            n.value = lerp(n.value, b.detector.smooth, dt * n.speed);
            let mut t = inverse_lerp(n.min_value, n.max_value, n.value + n.offset);
            if n.inv {
                t = 1. - t;
            }
            let mut euler = n.rest;
            euler[n.axis] = lerp(n.min_angle, n.max_angle, t);
            let rotation = unity_euler(euler);
            if rotation != n.pose.rotation {
                n.pose.rotation = rotation;
                let slot = ContentSlot::Active(b.slot);
                if !self.presentation_changes.contains(&slot) {
                    self.presentation_changes.push(slot);
                }
            }
        }
    }
    /// Marks the active content of placement `parent` for presentation. `hint` caches its slot.
    pub fn mark_presentation(&mut self, parent: &GeneratedId, hint: &mut usize) {
        if self
            .contents
            .active
            .get(*hint)
            .is_none_or(|c| &c.content.placement.id != parent)
        {
            let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| &c.content.placement.id == parent)
            else {
                return;
            };
            *hint = slot;
        }
        let slot = ContentSlot::Active(*hint);
        if !self.presentation_changes.contains(&slot) {
            self.presentation_changes.push(slot);
        }
    }
    pub fn detector(&self, parent: &GeneratedId) -> Option<&DetectorBinding> {
        self.detector_bindings.iter().find(|b| &b.parent == parent)
    }
}

#[cfg(test)]
impl WorldStream {
    /// Test probe of the detector on `item`: on, smooth value, needle value, the detector
    /// transform in physics space and the first switch knob.
    pub fn detector_probe(
        &self,
        item: ItemId,
        items: &ItemStore,
    ) -> Option<(bool, f32, f32, Pose, ColliderId)> {
        let b = self.detector_bindings.iter().find(|b| b.item == item)?;
        let transform = to_rapier(items.get(item)?.pose) * b.transform;
        Some((
            b.detector.on,
            b.detector.smooth,
            b.needle.value,
            transform,
            *b.knobs.first()?,
        ))
    }
}
