//! Car crash step: source `carscript.Update` calls `DamageStuff(|speed - lspeed|)`.
use super::{GarageRuntime, VehicleRuntime};
use crate::crash::{Blood, Candidate, Flags, HornHit};
use crate::interaction::cleaning::WashOwner;
use crate::interaction::items::{ItemLocation, ItemStore};
use crate::interaction::wheels::WheelAssemblies;
use crate::reference::{ItemId, ReferenceManifest};
use crate::simulation::FIXED_DT;
use crate::vehicle::PhysicsWorld;
use crate::world::ObjectKey;
use rapier3d::math::Vector;

/// Source `carscript.Stuff` speed: forward speed in km/h. The port has no `whlocked`.
pub(super) fn forward_kmh(physics: &PhysicsWorld, key: &ObjectKey) -> f32 {
    physics.snapshot(key).speed_mps.abs() * 3.6
}

/// True if `root` holds `item` through slots, wheel slots, attachments or storage locks.
fn under(items: &ItemStore, wheels: &WheelAssemblies, mut item: ItemId, root: ItemId) -> bool {
    for _ in 0..items.len() {
        let parent = match items.get(item).map(|i| i.location) {
            Some(ItemLocation::WheelSlot(slot)) => wheels.slot_owner(slot),
            _ => items.parent(item),
        };
        match parent {
            Some(parent) if parent == root => return true,
            Some(parent) => item = parent,
            None => return false,
        }
    }
    false
}

impl GarageRuntime {
    /// Runs the crash rule for every loaded car after the physics step.
    pub(super) fn step_crashes(&mut self, reference: &ReferenceManifest) {
        // Taking the map lets the fall-off borrow the runtime without allocation.
        let mut vehicles = std::mem::take(&mut self.vehicles);
        for (key, vehicle) in &mut vehicles {
            vehicle.horn_hit = vehicle.horn_hit.and_then(|h| h.advanced(FIXED_DT));
            let v = (forward_kmh(&self.physics, key) - vehicle.last_speed).abs();
            self.crash_car(reference, key, vehicle, v);
        }
        self.vehicles = vehicles;
    }

    /// Source `carscript.DamageStuff(v)` on car `key`: the crash step and blasts call it.
    /// The caller takes `vehicle` out of `self.vehicles`.
    pub(crate) fn crash_car(
        &mut self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
        vehicle: &mut VehicleRuntime,
        v: f32,
    ) {
        let record = vehicle.record(reference);
        let driver = self.area.player.driving() && self.area.player.seat_vehicle() == Some(key);
        let car = self.area.items.resolve_key(key);
        let condition = record
            .condition
            .and(car)
            .and_then(|car| self.area.interaction.conditions.item_state(car));
        let flags = Flags {
            driving: driver,
            player_driving: driver,
            unpaused: !self.paused,
            has_blood: vehicle.blood.is_some(),
            blood_on: vehicle.blood.as_ref().is_some_and(|b| b.on),
        };
        let outcome = crate::crash::damage(
            &reference.crash,
            &record.crash,
            v,
            flags,
            condition,
            &mut self.crash.random,
        );
        if let Some(time) = outcome.horn {
            vehicle.horn_hit = Some(HornHit::new(time));
        }
        // Source `player.Death()`: the blast death path.
        if outcome.kill {
            self.survival
                .damage_instant(&reference.survival, reference.survival.maximum.hp);
        }
        if let (true, Some(blood), Some(car)) = (outcome.blood, &mut vehicle.blood, car) {
            self.got_dirty(key, car, blood, record.blood_clip);
        }
        if let (Some((lerp, count)), Some(car)) = (outcome.fall_off, car) {
            self.candidates(reference, car, lerp);
            self.fall_off(count);
        }
    }

    /// Source `washtriggerscript.GotDirty` on the car blood, then `SBlood01` at the trigger.
    fn got_dirty(&mut self, key: &ObjectKey, car: ItemId, blood: &mut Blood, clip: Option<u32>) {
        blood.got_dirty();
        blood.set_colliders(&mut self.physics, true);
        self.area.interaction.cleaning.register_generated_wash(
            WashOwner::Car(car),
            blood.permission,
            blood.never_repair,
            blood.colliders.clone(),
        );
        let chassis = self.physics.rapier().bodies[self.physics.chassis_handle(key)].position();
        let point = chassis * Vector::from_array(blood.point);
        if let (Some(clip), Some(audio)) = (clip, &mut self.audio) {
            let position = self.stream.origin.absolute(point.to_array());
            audio.point_clip(clip, position);
        }
    }

    /// Source `washtriggerscript.GotClean` on the car blood that the sponge washed this frame.
    pub(super) fn clean_car_blood(&mut self) {
        let cleaning = &self.area.interaction.cleaning;
        for owner in cleaning.cleaned_washes() {
            let WashOwner::Car(car) = *owner else {
                continue;
            };
            let key = self.area.items.object_key(car);
            if let Some(blood) = self.vehicles.get_mut(&key).and_then(|v| v.blood.as_mut()) {
                blood.got_clean();
                blood.set_colliders(&mut self.physics, false);
            }
        }
    }

    /// Source `currentParts`: parts under the car that pass `WouldFallOff(crashLerp)`.
    fn candidates(&mut self, reference: &ReferenceManifest, car: ItemId, lerp: f32) {
        let (items, conditions, wheels) = (
            &self.area.items,
            &self.area.interaction.conditions,
            &self.wheels,
        );
        let passes = |item, category| {
            crate::crash::would_fall_off(
                &reference.crash,
                category,
                conditions.item_state(item),
                lerp,
            )
        };
        let parts = &self.area.interaction.parts;
        // The port keeps the player seated: a part that holds the player's seat stays on.
        let seat = self.area.player.seat_item();
        let holds_seat =
            |item| seat.is_some_and(|s| parts.holds(item, s) || under(items, wheels, s, item));
        let candidates = &mut self.crash.candidates;
        candidates.clear();
        for (item, category) in parts.mounted() {
            if under(items, wheels, item, car) && passes(item, category) && !holds_seat(item) {
                candidates.push(Candidate::Part(item));
            }
        }
        // Tires are category `gumi`, which never falls off, so only rims are candidates.
        for (slot, rim, category) in wheels.mounted_rims(car) {
            if passes(rim, category) {
                candidates.push(Candidate::Wheel(slot));
            }
        }
    }

    /// Source `FallOFf` on `count` random candidates.
    fn fall_off(&mut self, count: usize) {
        let interaction = &mut self.area.interaction;
        let (items, physics) = (&mut self.area.items, &mut self.physics);
        for _ in 0..count {
            if self.crash.candidates.is_empty() {
                break;
            }
            let len = self.crash.candidates.len() as i32;
            let index = self.crash.random.range_int(0, len) as usize;
            let item = match self.crash.candidates.remove(index) {
                Candidate::Part(item) => {
                    interaction
                        .parts
                        .fall_off(item, items, physics)
                        .expect("mounted part falls off");
                    interaction
                        .parts
                        .detach_mechanism(item, &mut interaction.mechanisms, physics);
                    interaction.sync_parts(items, physics).expect("bound parts");
                    item
                }
                Candidate::Wheel(slot) => self
                    .wheels
                    .fall_off(slot, items, physics)
                    .expect("mounted wheel falls off"),
            };
            interaction.storage.hold_till_pickup(item);
        }
    }
}
