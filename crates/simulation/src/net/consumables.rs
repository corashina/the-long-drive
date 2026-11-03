//! Source `SFood`, `SAmmo` and `SWeapon`: food bites, ammo counts and gun states. The source sends
//! at each change. The port reads them each `SYNC_TIME` and sends what changed since it last sent
//! or received it.
use super::items::Out;
use super::message::{AmmoMessage, FoodMessage, Message, WeaponMessage};
use super::objects::{NetKey, ObjectIndex};
use super::state::SYNC_TIME;
use crate::interaction::guns::GunSnapshot;
use crate::reference::{ItemId, ReferenceManifest};
use crate::starting_area::GarageRuntime;
use std::collections::HashMap;

#[derive(Default)]
pub struct ConsumableSync {
    timer: f32,
    foods: HashMap<ItemId, u8>,
    ammo: HashMap<ItemId, f32>,
    guns: HashMap<ItemId, GunSnapshot>,
}

impl ConsumableSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sends changes each `SYNC_TIME`, and the state of each `requested` item.
    pub(super) fn send(
        &mut self,
        runtime: &GarageRuntime,
        index: &ObjectIndex,
        requested: &[NetKey],
        dt: f32,
        out: &mut Out,
    ) {
        for &key in requested {
            if let Some(item) = index.item(key) {
                send_all(runtime, item, key, out);
            }
        }
        self.timer += dt;
        if self.timer < SYNC_TIME {
            return;
        }
        self.timer = 0.;
        // The first read of each item is the baseline: both peers start from one save.
        for state in runtime.area.items.iter() {
            if let Some(food) = state.food.filter(|_| !state.removed)
                && let Some(known) = self.foods.insert(state.id, food.bite)
                && known != food.bite
                && let Some(key) = index.key(state.id)
            {
                send_food(key, food.bite, out);
            }
        }
        let ammo = runtime
            .area
            .spawned
            .ammo_items()
            .chain(runtime.stream.ammo_items());
        for (item, a) in ammo {
            if let Some(known) = self.ammo.insert(item, a.ammo.count)
                && known != a.ammo.count
                && let Some(key) = index.key(item)
            {
                send_ammo(key, a.ammo.count, out);
            }
        }
        for (item, gun) in runtime.area.interaction.guns.states() {
            if let Some(known) = self.guns.insert(item, gun)
                && known != gun
                && let Some(key) = index.key(item)
            {
                send_gun(key, &gun, out);
            }
        }
        let items = &runtime.area.items;
        self.foods
            .retain(|id, _| items.get(*id).is_some_and(|i| !i.removed));
        self.ammo
            .retain(|id, _| items.get(*id).is_some_and(|i| !i.removed));
        self.guns
            .retain(|id, _| items.get(*id).is_some_and(|i| !i.removed));
    }

    /// Source `UpdFood`, `UpdAmmo` and `Weapon` of another peer.
    pub fn receive(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        index: &ObjectIndex,
        packet: &[u8],
    ) {
        let item = |key| index.item(NetKey(key));
        if let Ok((_, m)) = FoodMessage::decode(packet) {
            if let Some(id) = item(m.key) {
                self.got_food(reference, runtime, id, &m);
            }
        } else if let Ok((_, m)) = AmmoMessage::decode(packet) {
            if let Some(id) = item(m.key) {
                self.got_ammo(runtime, id, &m);
            }
        } else if let Ok((_, m)) = WeaponMessage::decode(packet)
            && let Some(id) = item(m.key)
        {
            self.got_gun(runtime, id, &m);
        }
    }

    /// Source `UpdFood`.
    fn got_food(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        item: ItemId,
        m: &FoodMessage,
    ) {
        let Ok(bite) = u8::try_from(m.bite) else {
            return;
        };
        if let Err(error) = runtime.set_food_bite(reference, item, bite) {
            runtime.stream.fail(error);
        }
        if let Some(food) = runtime.area.items.get(item).and_then(|s| s.food) {
            self.foods.insert(item, food.bite);
        }
    }

    /// Source `UpdAmmo`: `ammoscript.UpdAmmoFromMulti`.
    fn got_ammo(&mut self, runtime: &mut GarageRuntime, item: ItemId, m: &AmmoMessage) {
        let area = &runtime.area;
        let Some(mut ammo) = area
            .spawned
            .ammo(item)
            .or_else(|| runtime.stream.ammo_item(item))
        else {
            return;
        };
        let Some(count) = ammo.ammo.with_count(m.count) else {
            return;
        };
        ammo.ammo = count;
        runtime.set_ammo_item(item, ammo);
        self.ammo.insert(item, m.count);
    }

    /// Source `Weapon`: `weaponscript.UpdFromMulti`.
    fn got_gun(&mut self, runtime: &mut GarageRuntime, item: ItemId, m: &WeaponMessage) {
        let state = GunSnapshot {
            ammo: m.ammo,
            fr: m.fr,
            chambers: m.chambers,
            cylinder: m.cylinder,
            power: m.power,
            has_mag: m.has_mag != 0,
        };
        if runtime.area.interaction.guns.set(item, state) {
            self.guns.insert(item, state);
        }
    }
}

fn send_all(runtime: &GarageRuntime, item: ItemId, key: NetKey, out: &mut Out) {
    let area = &runtime.area;
    if let Some(food) = area.items.get(item).and_then(|s| s.food)
        && food.bite != 0
    {
        send_food(key, food.bite, out);
    }
    if let Some(a) = area
        .spawned
        .ammo(item)
        .or_else(|| runtime.stream.ammo_item(item))
    {
        send_ammo(key, a.ammo.count, out);
    }
    if let Some(gun) = area.interaction.guns.snapshot(item) {
        send_gun(key, &gun, out);
    }
}

fn send_food(key: NetKey, bite: u8, out: &mut Out) {
    out.push(&FoodMessage {
        key: key.0,
        bite: u32::from(bite),
        pad: 0,
    });
}

fn send_ammo(key: NetKey, count: f32, out: &mut Out) {
    out.push(&AmmoMessage {
        key: key.0,
        count,
        pad: 0,
    });
}

fn send_gun(key: NetKey, gun: &GunSnapshot, out: &mut Out) {
    out.push(&WeaponMessage {
        key: key.0,
        ammo: gun.ammo,
        fr: gun.fr,
        chambers: gun.chambers,
        cylinder: gun.cylinder,
        power: gun.power,
        has_mag: u32::from(gun.has_mag),
    });
}
