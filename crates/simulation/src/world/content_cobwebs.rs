//! Source washtriggerscript cobwebs on generated structures.
//! Building content regenerates, so cobwebs are not saved.
use super::WorldStream;
use crate::interaction::InteractionState;
use crate::interaction::cleaning::{CleaningTools, ToolFrame, WashOwner};
use crate::particles::ParticleState;
use crate::reference::{ColliderId, ReferenceError, ReferenceManifest, SourceId};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::ObjectKey;
use crate::world::unity_random::UnityRandom;
use rapier3d::prelude::ColliderHandle;
use std::collections::HashMap;

struct Cobweb {
    id: u32,
    parent: GeneratedId,
    /// Index in `small_items.washes`.
    wash: usize,
    colliders: Vec<ColliderHandle>,
}
/// One particleOnDestroy prefab. Each destroy restarts it at the trigger.
struct Effect {
    particle: ParticleState,
    duration: f32,
    elapsed: Option<f32>,
    emitter: [f64; 3],
    clip: Option<u32>,
    volume: f32,
}
pub struct Cobwebs {
    webs: Vec<Cobweb>,
    next: u32,
    scan: Option<(u64, u64)>,
    /// Wash records by prototype.
    records: HashMap<SourceId, Vec<usize>>,
    /// Inactive prototype nodes by structure.
    hidden: HashMap<GeneratedId, Vec<usize>>,
    effects: Vec<Effect>,
    /// Structures whose hidden nodes changed.
    pub(super) changes: Vec<GeneratedId>,
}
impl Cobwebs {
    pub fn new(reference: &ReferenceManifest) -> Self {
        let mut records: HashMap<SourceId, Vec<usize>> = HashMap::new();
        for (i, w) in reference.small_items.washes.iter().enumerate() {
            records.entry(w.prototype.clone()).or_default().push(i);
        }
        let effects = reference
            .small_items
            .wash_effects
            .iter()
            .map(|e| Effect {
                particle: ParticleState::new(&e.particle, e.source.path_id as u64),
                duration: e.particle.duration,
                elapsed: None,
                emitter: [0.; 3],
                clip: reference.clip_id(&e.clip),
                volume: e.volume,
            })
            .collect();
        Self {
            webs: Vec::new(),
            next: 0,
            scan: None,
            records,
            hidden: HashMap::new(),
            effects,
            changes: Vec::new(),
        }
    }
    pub fn heap_bytes(&self) -> usize {
        self.webs.capacity() * std::mem::size_of::<Cobweb>()
            + self
                .webs
                .iter()
                .map(|w| w.colliders.capacity() * std::mem::size_of::<ColliderHandle>())
                .sum::<usize>()
            + self
                .hidden
                .values()
                .map(|n| n.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
    }
    pub fn particles(&self, index: usize) -> &[crate::particles::ParticleOutput] {
        self.effects[index].particle.particles()
    }
}

impl WorldStream {
    /// Registers the wash triggers of new structures and drops the ones of removed structures.
    pub(super) fn sync_cobwebs(
        &mut self,
        reference: &ReferenceManifest,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.structure_revision);
        if self.cobwebs.scan == Some(revision) {
            return Ok(());
        }
        let structures = &self.structures;
        let cobwebs = &mut self.cobwebs;
        let present = |parent: &GeneratedId| structures.iter().any(|s| &s.placement.id == parent);
        cobwebs.webs.retain(|w| {
            let keep = present(&w.parent);
            if !keep {
                interaction
                    .cleaning
                    .remove_generated_wash(WashOwner::Cobweb(w.id));
            }
            keep
        });
        cobwebs.hidden.retain(|parent, _| present(parent));
        for s in structures {
            let Some(records) = cobwebs.records.get(&s.placement.prototype) else {
                continue;
            };
            let id = &s.placement.id;
            if cobwebs.webs.iter().any(|w| &w.parent == id) || cobwebs.hidden.contains_key(id) {
                continue;
            }
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == s.placement.prototype)
                .ok_or_else(|| ReferenceError("Cobweb prototype missing".into()))?;
            let mut random = UnityRandom::new(ObjectKey::Generated(id.clone()).seed() as i32);
            let mut hidden = Vec::new();
            for &wash in records {
                let w = &reference.small_items.washes[wash];
                let node = prototype
                    .nodes
                    .iter()
                    .position(|n| n.source == w.node)
                    .ok_or_else(|| ReferenceError("Cobweb node outside its prototype".into()))?;
                let colliders: Vec<ColliderHandle> = s
                    .colliders
                    .iter()
                    .filter(|(source, _)| w.colliders.contains(source))
                    .map(|(_, h)| *h)
                    .collect();
                // washtriggerscript.Start: Random.Range(0f, 1f) < chanceToStartDisabled.
                if w.chance_to_start_disabled > 0.
                    && random.range(0., 1.) < w.chance_to_start_disabled
                {
                    hidden.push(node);
                    for h in &colliders {
                        physics.edit_collider(*h, |c| c.set_enabled(false));
                    }
                    continue;
                }
                let mut ids = Vec::with_capacity(colliders.len());
                for h in &colliders {
                    let mut collider =
                        ColliderId((physics.rapier().colliders[*h].user_data >> 64) as u32);
                    if collider.0 == 0 {
                        collider = interaction.allocate_generated_collider()?;
                        physics.edit_collider(*h, |c| c.user_data |= (collider.0 as u128) << 64);
                    }
                    ids.push((collider, *h));
                }
                let web = cobwebs.next;
                cobwebs.next = web
                    .checked_add(1)
                    .ok_or_else(|| ReferenceError("Cobweb identity exhausted".into()))?;
                interaction.cleaning.register_generated_wash(
                    WashOwner::Cobweb(web),
                    w.permission,
                    w.never_repair,
                    ids,
                );
                cobwebs.webs.push(Cobweb {
                    id: web,
                    parent: id.clone(),
                    wash,
                    colliders,
                });
            }
            if !hidden.is_empty() {
                cobwebs.changes.push(id.clone());
            }
            cobwebs.hidden.insert(id.clone(), hidden);
        }
        cobwebs.scan = Some(revision);
        Ok(())
    }
    /// washtriggerscript.GotClean for the cleaned cobwebs, then the destroy particles.
    pub fn advance_cobwebs(
        &mut self,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        cleaning: &mut CleaningTools,
        physics: &mut PhysicsWorld,
    ) {
        for owner in cleaning.take_cleaned_washes() {
            let WashOwner::Cobweb(id) = owner else {
                continue;
            };
            let Some(i) = self.cobwebs.webs.iter().position(|w| w.id == id) else {
                continue;
            };
            let web = self.cobwebs.webs.swap_remove(i);
            let w = &reference.small_items.washes[web.wash];
            let Some(s) = self
                .structures
                .iter()
                .find(|s| s.placement.id == web.parent)
            else {
                continue;
            };
            let Some(prototype) = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == s.placement.prototype)
            else {
                continue;
            };
            let index =
                |source: &SourceId| prototype.nodes.iter().position(|n| &n.source == source);
            let hidden = self.cobwebs.hidden.entry(web.parent.clone()).or_default();
            hidden.extend(index(&w.node));
            hidden.extend(w.objects.iter().filter_map(index));
            for h in &web.colliders {
                physics.edit_collider(*h, |c| c.set_enabled(false));
            }
            if !self.cobwebs.changes.contains(&web.parent) {
                self.cobwebs.changes.push(web.parent.clone());
            }
            if !w.destroy_on_repair {
                continue;
            }
            let (Some(node), Some(effect)) = (
                index(&w.node),
                w.particle_on_destroy.as_ref().and_then(|e| {
                    reference
                        .small_items
                        .wash_effects
                        .iter()
                        .position(|f| &f.source == e)
                }),
            ) else {
                continue;
            };
            let pose = crate::world::placement::place_pose(
                s.placement.pose,
                s.placement.scale,
                crate::world::placement::node_pose(prototype, node),
            );
            let e = &mut self.cobwebs.effects[effect];
            e.particle.restart();
            e.particle.reset_emitter();
            e.elapsed = Some(0.);
            e.emitter = pose.position;
            if let Some(clip) = e.clip {
                self.small_items
                    .sounds
                    .push((clip, e.volume, pose.position));
            }
        }
        let dt = frame.dt;
        if !frame.active || !dt.is_finite() || dt <= 0. {
            return;
        }
        for e in &mut self.cobwebs.effects {
            let Some(elapsed) = e.elapsed.as_mut() else {
                continue;
            };
            e.particle.set_origin(frame.origin);
            e.particle.advance(
                dt,
                e.emitter,
                if *elapsed < e.duration { 1. } else { 0. },
                physics,
            );
            *elapsed += dt;
        }
    }
    /// Prototype nodes that washtriggerscript deactivated on this structure.
    pub fn cobweb_hidden(&self, parent: &GeneratedId) -> &[usize] {
        self.cobwebs.hidden.get(parent).map_or(&[], Vec::as_slice)
    }
    pub fn cobweb_changes(&self) -> &[GeneratedId] {
        &self.cobwebs.changes
    }
    pub fn cobweb_particles(&self, index: usize) -> &[crate::particles::ParticleOutput] {
        self.cobwebs.particles(index)
    }
}
