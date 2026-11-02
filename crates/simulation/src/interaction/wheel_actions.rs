use super::{
    InteractionAction, InteractionState, ResolvedAim,
    items::{ItemLocation, ItemStore},
    wheels::{WheelAssemblies, WheelSlotId},
};
use crate::{
    player::PlayerState,
    reference::{ItemId, ReferenceError},
    vehicle::PhysicsWorld,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelAction {
    Remove(ItemId),
    Install { item: ItemId, target: WheelSlotId },
    Split(ItemId),
    Join { rim: ItemId, tire: ItemId },
}

impl InteractionState {
    pub fn complete_wheel_action(
        &mut self,
        action: WheelAction,
        aim: ResolvedAim,
        wheels: &mut WheelAssemblies,
        player: &PlayerState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Option<&'static str> {
        Some(match action {
            WheelAction::Remove(item) => {
                let slot = wheels.slot_for_item(item)?;
                match wheels.remove(slot, items, physics) {
                    Ok(rim) => {
                        self.completed_action(InteractionAction::Wheel(WheelAction::Remove(rim)));
                        let point = aim
                            .grab_target
                            .map_or(items.get(rim).unwrap().pose.position, |target| target.point);
                        match self.pickup_item(rim, point, player, items, physics) {
                            Ok(()) => "",
                            Err(_) => "Cannot carry this wheel.",
                        }
                    }
                    Err(_) => "Cannot remove this wheel.",
                }
            }
            WheelAction::Install { item, target } => {
                if wheels.can_install(item, target, items).is_err() {
                    return Some("This wheel does not fit the slot.");
                }
                self.drop_held(items, physics);
                if wheels.install(item, target, items, physics).is_ok() {
                    self.completed_action(InteractionAction::Wheel(WheelAction::Install {
                        item,
                        target,
                    }));
                    if let Some(sensor) = wheels.sensor(target) {
                        self.bind_wheel_sensor(sensor, item, physics);
                    }
                    ""
                } else {
                    "This wheel does not fit the slot."
                }
            }
            WheelAction::Split(rim) => {
                let point = aim
                    .grab_target
                    .or(aim.focus_target)
                    .map_or(items.get(rim).unwrap().pose.position, |target| target.point);
                match self.split_wheel(rim, point, player, wheels, items, physics) {
                    Ok(tire) => {
                        self.completed_action(InteractionAction::PartRemove(tire));
                        ""
                    }
                    Err(_) => "Cannot dismount this tire.",
                }
            }
            WheelAction::Join { rim, tire } => {
                if self.join_wheel(rim, tire, wheels, items, physics).is_ok() {
                    self.completed_action(InteractionAction::Wheel(WheelAction::Join {
                        rim,
                        tire,
                    }));
                    ""
                } else {
                    "This tire does not fit the rim."
                }
            }
        })
    }

    pub fn join_wheel(
        &mut self,
        rim: ItemId,
        tire: ItemId,
        wheels: &mut WheelAssemblies,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if self.held() != Some(tire)
            || items
                .get(rim)
                .is_none_or(|item| item.location != ItemLocation::World)
        {
            return Err(ReferenceError("Hold the tire beside a loose rim".into()));
        }
        wheels.can_assemble(rim, tire, items)?;
        self.drop_held(items, physics);
        wheels.assemble(rim, tire, items, physics)?;
        Ok(())
    }

    pub fn split_wheel(
        &mut self,
        rim: ItemId,
        point: [f32; 3],
        player: &PlayerState,
        wheels: &mut WheelAssemblies,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        if self.held().is_some()
            || items
                .get(rim)
                .is_none_or(|item| item.location != ItemLocation::World)
        {
            return Err(ReferenceError(
                "Release the wheel before tire removal".into(),
            ));
        }
        let tire = wheels.can_split(rim, items)?;
        if !self.carry.allows_pickup(tire, player.seated(), true) {
            return Err(ReferenceError("Cannot remove this tire".into()));
        }
        let body = items.get(rim).unwrap().body.unwrap();
        self.carry.can_acquire(tire, point, player, body, physics)?;
        wheels.split(rim, items, physics)?;
        self.pickup_item(tire, point, player, items, physics)?;
        Ok(tire)
    }
}
