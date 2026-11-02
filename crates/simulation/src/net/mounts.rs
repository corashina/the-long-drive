//! Source `partslotscript.Craft` and `UnCraft` (`SPartSlot`) and `attachablescript.Attach` and
//! `Detach` (`SAttachable`): parts, wheels, tires and trailer grips. The source sends at each change.
//! The port reads the mounts each `SYNC_TIME` and sends what changed since it last sent or received it.
use super::inventory::drop_local;
use super::items::Out;
use super::message::MountMessage;
use super::objects::{NetKey, ObjectIndex};
use super::state::SYNC_TIME;
use crate::interaction::items::ItemLocation;
use crate::reference::{ItemId, ReferenceError};
use crate::starting_area::GarageRuntime;
use crate::world::source_seed;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mount {
    Loose,
    Part { owner: Option<ItemId>, seed: u64 },
    Wheel { owner: Option<ItemId>, seed: u64 },
    Tire(ItemId),
    Hitch { owner: ItemId, seed: u64 },
}

#[derive(Default)]
pub struct MountSync {
    timer: f32,
    started: bool,
    /// Mounted items as last sent or received.
    known: HashMap<ItemId, Mount>,
    current: HashMap<ItemId, Mount>,
}

impl MountSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sends changed mounts each `SYNC_TIME`, and the mount of each `requested` mountable item.
    pub(super) fn send(
        &mut self,
        runtime: &GarageRuntime,
        index: &ObjectIndex,
        requested: &[NetKey],
        dt: f32,
        out: &mut Out,
    ) {
        for &key in requested {
            if let Some(item) = index.item(key)
                && mountable(runtime, item)
            {
                send(index, key, mount_of(runtime, item), out);
            }
        }
        self.timer += dt;
        if self.timer < SYNC_TIME {
            return;
        }
        self.timer = 0.;
        read(runtime, &mut self.current);
        // The first read is the baseline: both peers start from one save.
        if std::mem::replace(&mut self.started, true) {
            for (item, mount) in &self.current {
                if self.known.get(item) != Some(mount)
                    && let Some(key) = index.key(*item)
                {
                    send(index, key, *mount, out);
                }
            }
            for item in self.known.keys() {
                if !self.current.contains_key(item)
                    && runtime.area.items.get(*item).is_some_and(|i| !i.removed)
                    && let Some(key) = index.key(*item)
                {
                    send(index, key, Mount::Loose, out);
                }
            }
        }
        std::mem::swap(&mut self.known, &mut self.current);
    }

    /// Source `PartSlotCraft`, `PartSlotUnCraft` and `UpdAttachable`.
    pub fn got(
        &mut self,
        runtime: &mut GarageRuntime,
        index: &ObjectIndex,
        item: ItemId,
        m: &MountMessage,
    ) {
        let owner = match m.owner {
            0 => None,
            key => match index.item(NetKey(key)) {
                Some(owner) => Some(owner),
                None => return,
            },
        };
        let seed = m.seed;
        let want = match (m.kind, owner) {
            (MountMessage::LOOSE, _) => Mount::Loose,
            (MountMessage::PART, owner) => Mount::Part { owner, seed },
            (MountMessage::WHEEL, owner) => Mount::Wheel { owner, seed },
            (MountMessage::TIRE, Some(rim)) => Mount::Tire(rim),
            (MountMessage::HITCH, Some(owner)) => Mount::Hitch { owner, seed },
            _ => return,
        };
        let now = mount_of(runtime, item);
        if now != want {
            drop_local(runtime, item);
            if unmount(runtime, item, now).is_ok() {
                let _ = mount(runtime, item, want);
            }
            runtime.area.items.mark_dirty(item);
        }
        // A failed mount keeps the local state quiet: it does not send it back.
        match mount_of(runtime, item) {
            Mount::Loose => self.known.remove(&item),
            mount => self.known.insert(item, mount),
        };
    }
}

/// Parts, wheel parts and trailer grips.
fn mountable(runtime: &GarageRuntime, item: ItemId) -> bool {
    let interaction = &runtime.area.interaction;
    interaction.parts.contains(item)
        || interaction.hitch.is_grip(item)
        || runtime.wheels.part_mass(item).is_some()
}

fn location_mount(runtime: &GarageRuntime, location: ItemLocation) -> Option<Mount> {
    let interaction = &runtime.area.interaction;
    match location {
        ItemLocation::PartSlot(slot) => {
            let (owner, source) = interaction.parts.slot_identity(slot)?;
            Some(Mount::Part {
                owner,
                seed: source_seed(source),
            })
        }
        ItemLocation::WheelSlot(slot) => {
            let (owner, source) = interaction.wheel_slot_identity(slot)?;
            Some(Mount::Wheel {
                owner,
                seed: source_seed(source),
            })
        }
        ItemLocation::Attached(rim) => Some(Mount::Tire(rim)),
        _ => None,
    }
}

fn mount_of(runtime: &GarageRuntime, item: ItemId) -> Mount {
    if let Some(mount) = runtime
        .area
        .items
        .get(item)
        .and_then(|i| location_mount(runtime, i.location))
    {
        return mount;
    }
    match runtime.area.interaction.hitch.hitched_target(item) {
        Some((owner, source)) => Mount::Hitch {
            owner,
            seed: source_seed(source),
        },
        None => Mount::Loose,
    }
}

fn read(runtime: &GarageRuntime, out: &mut HashMap<ItemId, Mount>) {
    out.clear();
    for state in runtime.area.items.iter() {
        if !state.removed
            && let Some(mount) = location_mount(runtime, state.location)
        {
            out.insert(state.id, mount);
        }
    }
    for (grip, owner, source) in runtime.area.interaction.hitch.hitched() {
        let seed = source_seed(source);
        out.insert(grip, Mount::Hitch { owner, seed });
    }
}

fn send(index: &ObjectIndex, key: NetKey, mount: Mount, out: &mut Out) {
    let owner_key = |owner: Option<ItemId>| match owner {
        Some(owner) => index.key(owner).map(|k| k.0),
        None => Some(0),
    };
    let (owner, seed, kind) = match mount {
        Mount::Loose => (Some(0), 0, MountMessage::LOOSE),
        Mount::Part { owner, seed } => (owner_key(owner), seed, MountMessage::PART),
        Mount::Wheel { owner, seed } => (owner_key(owner), seed, MountMessage::WHEEL),
        Mount::Tire(rim) => (owner_key(Some(rim)), 0, MountMessage::TIRE),
        Mount::Hitch { owner, seed } => (owner_key(Some(owner)), seed, MountMessage::HITCH),
    };
    let Some(owner) = owner else {
        return;
    };
    out.push(&MountMessage {
        key: key.0,
        owner,
        seed,
        kind,
        pad: 0,
    });
}

fn unmount(runtime: &mut GarageRuntime, item: ItemId, mount: Mount) -> Result<(), ReferenceError> {
    let area = &mut runtime.area;
    let (items, physics) = (&mut area.items, &mut runtime.physics);
    match mount {
        Mount::Loose => Ok(()),
        Mount::Part { .. } => area.interaction.fall_off_part(item, items, physics),
        Mount::Wheel { .. } => {
            let Some(ItemLocation::WheelSlot(slot)) = items.get(item).map(|i| i.location) else {
                return Err(ReferenceError("Wheel is not mounted".into()));
            };
            runtime.wheels.fall_off(slot, items, physics).map(drop)
        }
        Mount::Tire(rim) => runtime.wheels.split(rim, items, physics).map(drop),
        Mount::Hitch { .. } => area.interaction.hitch.detach(item, items, physics),
    }
}

fn mount(runtime: &mut GarageRuntime, item: ItemId, mount: Mount) -> Result<(), ReferenceError> {
    let area = &mut runtime.area;
    let interaction = &mut area.interaction;
    let (items, physics) = (&mut area.items, &mut runtime.physics);
    let missing = || ReferenceError("Missing mount target".into());
    match mount {
        Mount::Loose => Ok(()),
        Mount::Part { owner, seed } => {
            let slot = interaction.parts.slot_of(owner, seed).ok_or_else(missing)?;
            interaction.install_part(item, slot, items, physics)
        }
        Mount::Wheel { owner, seed } => {
            let slot = interaction.wheel_slot_of(owner, seed).ok_or_else(missing)?;
            runtime.wheels.install(item, slot, items, physics)?;
            if let Some(sensor) = runtime.wheels.sensor(slot) {
                interaction.bind_wheel_sensor(sensor, item, physics);
            }
            Ok(())
        }
        Mount::Tire(rim) => runtime.wheels.assemble(rim, item, items, physics).map(drop),
        Mount::Hitch { owner, seed } => {
            let target = interaction
                .hitch
                .target_of(owner, seed)
                .ok_or_else(missing)?;
            interaction.hitch.attach(item, target, items, physics)
        }
    }
}
