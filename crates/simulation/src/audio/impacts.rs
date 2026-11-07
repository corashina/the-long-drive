//! Source hit sounds: `pickupable.OnCollisionEnter` and `carscript.OnCollisionEnter`.
use super::{AudioAction, AudioKind, AudioState, SurvivalAudioEvent};
use crate::interaction::items::{ItemLocation, ItemState, ItemStore};
use crate::reference::{ColliderId, CollisionAudioReference, ItemId, ReferenceManifest};
use crate::round_robin::RoundRobin;
use crate::starting_area::StartingArea;
use crate::vehicle::{PhysicsWorld, VehiclePose, to_rapier};
use rapier3d::prelude::{Ball, QueryFilter};
use std::collections::HashSet;

type Vehicles = std::collections::BTreeMap<crate::world::ObjectKey, crate::car::VehicleRuntime>;

/// Played item hit gain. It is `None` below the source minimum or for a loose item in an interior.
pub fn item_hit_gain(
    speed: f32,
    gain: f32,
    held: bool,
    interior: bool,
    rules: &CollisionAudioReference,
) -> Option<f32> {
    if interior && !held {
        return None;
    }
    let g = speed * gain * if held { 1. } else { rules.loose_multiplier };
    (g > rules.item_minimum).then_some(g - rules.item_minimum)
}

/// Clip and gain of a car hit from the highest tier below the impact. `sample` is in 0..1.
pub fn car_hit(
    speed: f32,
    gain: f32,
    rules: &CollisionAudioReference,
    sample: f32,
) -> Option<(u32, f32)> {
    let m = speed * gain * rules.car_multiplier;
    let tier = rules.car_tiers.iter().rev().find(|t| m > t.threshold)?;
    let index = ((sample * tier.clips.len() as f32) as usize).min(tier.clips.len() - 1);
    Some((tier.clips[index], tier.fixed_gain.unwrap_or(m)))
}

/// Clip and gain of an item hit. Items keep their source hit sound in the item store.
fn item_hit(
    state: &ItemState,
    speed: f32,
    interior: bool,
    rules: &CollisionAudioReference,
) -> Option<(u32, f32)> {
    let sound = state.hit_sound?;
    let held = state.location == ItemLocation::Held;
    let gain = item_hit_gain(speed, sound.gain, held, interior, rules)?;
    Some((sound.clip, gain))
}

/// Car hit gains and the source interior flags of pickup items.
#[derive(Default)]
pub struct ImpactState {
    /// Items that the last interior test found in an interior.
    interior: HashSet<ItemId>,
    /// The interior test visits a share of the items each step.
    scan: RoundRobin,
}

#[derive(Clone, Copy)]
enum Side {
    Item(ItemId),
    /// A car chassis: its source `collisionSoundM` and its emitter.
    Car(f32, u32),
    Other,
}

impl ImpactState {
    pub fn bind(&mut self) {
        self.interior = HashSet::with_capacity(256);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.scan.reset();
        self.interior.clear();
    }
    /// Test each loose pickup once per `period` seconds, a share of the items each step.
    /// Held and stored items keep their flag.
    fn refresh(
        &mut self,
        dt: f32,
        period: f32,
        items: &ItemStore,
        loose: impl Fn(&ItemState) -> bool,
        mut inside: impl FnMut([f32; 3]) -> bool,
    ) {
        let range = self.scan.take(items.len(), (period / dt).round() as usize);
        if range.start == 0 {
            self.interior.retain(|&id| items.get(id).is_some());
        }
        for item in items.iter().skip(range.start).take(range.len()) {
            if !loose(item) {
                continue;
            }
            if inside(item.pose.position) {
                self.interior.insert(item.id);
            } else {
                self.interior.remove(&item.id);
            }
        }
    }
    fn interior(&self, id: ItemId, held: bool) -> bool {
        !held && self.interior.contains(&id)
    }
}

fn side(
    game: &ReferenceManifest,
    vehicles: &Vehicles,
    physics: &PhysicsWorld,
    handle: crate::vehicle::ColliderHandle,
) -> Side {
    let Some(collider) = physics.rapier().colliders.get(handle) else {
        return Side::Other;
    };
    if let Some((key, car)) = collider
        .parent()
        .and_then(|b| physics.vehicle_of(b))
        .and_then(|key| vehicles.get_key_value(key))
    {
        return Side::Car(car.record(game).collision_gain, key.seed() as u32);
    }
    match (collider.user_data & u32::MAX as u128) as u32 {
        0 => Side::Other,
        id => Side::Item(ItemId(id - 1)),
    }
}

impl AudioState {
    /// Emits hit sounds for the contacts of the last physics step.
    pub fn hits(
        &mut self,
        game: &ReferenceManifest,
        area: &StartingArea,
        vehicles: &Vehicles,
        physics: &PhysicsWorld,
        dt: f32,
    ) {
        let rules = &self.reference.collisions;
        let ball = Ball::new(rules.interior_radius);
        let colliders = &self.colliders;
        self.impacts.refresh(
            dt,
            rules.interior_check_seconds,
            &area.items,
            |s| {
                s.location == ItemLocation::World
                    && !s.removed
                    && area.interaction.pickup_rule(s.id).is_some()
            },
            |position| {
                let pose = to_rapier(VehiclePose {
                    position,
                    rotation: [0., 0., 0., 1.],
                });
                physics
                    .queries(QueryFilter::default())
                    .intersect_shape(pose, &ball)
                    .any(|(_, c)| colliders.contains_key(&ColliderId((c.user_data >> 64) as u32)))
            },
        );
        let held = |id: ItemId| {
            area.items
                .get(id)
                .is_some_and(|s| s.location == ItemLocation::Held)
        };
        for hit in physics.hits() {
            let sides = hit.colliders.map(|h| side(game, vehicles, physics, h));
            let position = std::array::from_fn(|i| area.origin[i] + f64::from(hit.point[i]));
            for (this, other) in [(sides[0], sides[1]), (sides[1], sides[0])] {
                let (kind, emitter, clip, gain, pitch) = match this {
                    Side::Item(id) => {
                        let Some(state) = area.items.get(id) else {
                            continue;
                        };
                        let interior = self.impacts.interior.contains(&id);
                        let rules = &self.reference.collisions;
                        let Some((clip, gain)) = item_hit(state, hit.speed, interior, rules) else {
                            continue;
                        };
                        (AudioKind::ItemHit, 0, clip, gain, 1.)
                    }
                    Side::Car(gain, emitter) => {
                        if let Side::Item(id) = other
                            && self.impacts.interior(id, held(id))
                        {
                            continue;
                        }
                        let sample = self.sample();
                        let [low, high] = self.reference.shared.random_pitch;
                        let pitch = low + (high - low) * self.sample();
                        let Some((clip, gain)) =
                            car_hit(hit.speed, gain, &self.reference.collisions, sample)
                        else {
                            continue;
                        };
                        (AudioKind::CarHit, emitter, clip, gain, pitch)
                    }
                    Side::Other => continue,
                };
                self.survival_event(SurvivalAudioEvent {
                    session: self.session,
                    emitter,
                    kind,
                    action: AudioAction::Start,
                    clip,
                    gain,
                    pitch,
                    position,
                });
            }
        }
    }
}
