use super::{
    AimTarget, InteractionAction, InteractionState,
    feedback::{ActionAvailability, ActionMask},
};
use crate::simulation::{FrameInput, keys};
use crate::{player::PlayerState, vehicle::PhysicsWorld};
use rapier3d::math::Vector;

#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionOutcome {
    pub action: Option<InteractionAction>,
    pub look_mouse: [f32; 2],
}

pub(super) struct ResolvedInput {
    pub use_input: FrameInput,
    pub grab_input: FrameInput,
    pub carry_input: FrameInput,
    pub push_input: FrameInput,
    pub look_mouse: [f32; 2],
    carry_mouse_owned: bool,
    carry_wheel_owned: bool,
}
fn remove(input: &mut FrameInput, keys: u32) {
    input.held &= !keys;
    input.pressed &= !keys;
    input.released &= !keys;
}
impl InteractionState {
    pub(super) fn active_mechanism_target(
        &self,
        player: &PlayerState,
        physics: &PhysicsWorld,
    ) -> Option<AimTarget> {
        let (item, body_id, local, collider) = self.latched_mechanism?;
        if !self.mechanisms.contains(item) {
            return None;
        }
        let body = physics.rapier().bodies.get(*self.bodies.get(&body_id)?)?;
        let point = *body.position() * local;
        let distance =
            point.distance_squared(Vector::from_array(player.body_pose(physics).position));
        (distance < self.mechanism_release_distance_squared).then_some(AimTarget {
            item,
            collider,
            point: point.to_array(),
            distance: distance.sqrt(),
            slot: None,
        })
    }
    pub fn filter_cancelled_input(&mut self, mut input: FrameInput) -> FrameInput {
        // A new edge starts a fresh gesture after pause, recovery, or input loss.
        self.cancelled_input &= !input.pressed;
        let blocked = self.cancelled_input;
        self.cancelled_input &= input.held;
        remove(&mut input, blocked);
        if blocked & (keys::RECENTER | keys::TRANSLATE | keys::ROTATE) != 0 {
            input.wheel = 0.;
        }
        input
    }
    pub(super) fn drag_input(&self, input: FrameInput, available: &ActionAvailability) -> bool {
        available.use_item.is_some_and(|item| {
            self.mechanisms.drag_eligible(item)
                && (input.held & keys::USE != 0
                    || (available.lmb_use
                        && self.held_light_control().is_none()
                        && input.held & keys::PRIMARY != 0)
                    || (input.wheel != 0. && self.mechanisms.scroll_eligible(item)))
        })
    }
    pub fn route_frame_input(
        &self,
        mut input: FrameInput,
        available: &ActionAvailability,
        player: &PlayerState,
    ) -> FrameInput {
        // Raw samples seed this channel once at the frame boundary.
        input.carry = crate::simulation::CarryDeltas {
            owner: self.held(),
            mouse: input.mouse,
            wheel: input.wheel,
        };
        let mut resolved = self.resolve_input(input, available);
        if player.seated() && input.player_held & crate::simulation::player_keys::LEAN != 0 {
            // Seated lean can share this sample with carry distance. The view step
            // updates the ray target after this route, so prior-frame tuning is stale.
            resolved.carry_input.wheel = input.wheel;
        }
        let view_owns_wheel = input.player_held & crate::simulation::player_keys::LEAN != 0
            || player.view_owns_look_input();
        input.carry = if let Some(owner) = self.held() {
            crate::simulation::CarryDeltas {
                owner: Some(owner),
                mouse: if input.held & (keys::TRANSLATE | keys::ROTATE) != 0 {
                    resolved.carry_input.mouse
                } else {
                    [0.; 2]
                },
                wheel: resolved.carry_input.wheel,
            }
        } else {
            crate::simulation::CarryDeltas::default()
        };
        if resolved.carry_mouse_owned {
            input.mouse = [0.; 2];
        }
        if resolved.carry_wheel_owned && !view_owns_wheel {
            input.wheel = 0.;
        }
        input
    }
    pub(super) fn resolve_input(
        &self,
        input: FrameInput,
        available: &ActionAvailability,
    ) -> ResolvedInput {
        let carry = if input.carry.owner.is_some() && input.carry.owner == self.held() {
            input.carry
        } else {
            crate::simulation::CarryDeltas::default()
        };
        let mut resolved = ResolvedInput {
            use_input: input,
            grab_input: input,
            carry_input: FrameInput {
                mouse: carry.mouse,
                wheel: carry.wheel,
                ..input
            },
            push_input: input,
            look_mouse: input.mouse,
            carry_mouse_owned: false,
            carry_wheel_owned: self.held().is_some(),
        };
        remove(&mut resolved.carry_input, keys::RECENTER);
        let primary_use = available.lmb_use && self.held_light_control().is_none();
        let use_held =
            input.held & keys::USE != 0 || (primary_use && input.held & keys::PRIMARY != 0);
        let drag = self.drag_input(input, available);
        let active_drag = available.active_drag && drag;
        let mounting = !self.carry.acquisition_held()
            && ((available.actions.0 & ActionMask::MOUNT != 0
                && input.released & keys::GRAB != 0
                && !self.f_gesture_consumed
                && self.carry.throw_charge() < self.carry.reference_throw_minimum())
                || (available.actions.0 & ActionMask::DISMOUNT != 0
                    && input.pressed & keys::GRAB != 0
                    && !self.push.gripping()));
        if active_drag {
            // RayStuff skips new F targets. Existing carry and grip release run separately.
            if self.held().is_none() && !self.push.gripping() {
                remove(&mut resolved.grab_input, keys::GRAB);
            }
            remove(&mut resolved.push_input, keys::GRAB);
        } else if mounting {
            remove(&mut resolved.use_input, keys::USE | keys::PRIMARY);
            resolved.use_input.mouse = [0.; 2];
            resolved.use_input.wheel = 0.;
        }
        let tuning = available.actions.0 & ActionMask::TUNE != 0;
        let tuning_mouse = tuning && use_held;
        if (drag || tuning_mouse) && !mounting || active_drag {
            resolved.look_mouse = [0.; 2];
            resolved.carry_input.mouse = [0.; 2];
        } else if self.held().is_some() && input.held & (keys::TRANSLATE | keys::ROTATE) != 0 {
            resolved.look_mouse = [0.; 2];
            resolved.carry_mouse_owned = true;
        }
        resolved.carry_wheel_owned &=
            available.actions.0 & (ActionMask::TUNE | ActionMask::SCROLL) == 0;
        // Source Carry permits RMB to share wheel input with tuning. MMB always blocks it.
        if input.held & keys::ROTATE != 0 || (tuning && input.held & keys::TRANSLATE == 0) {
            resolved.carry_input.wheel = 0.;
        }
        if input.pressed & keys::PRIMARY != 0 && self.held_light_control().is_some() {
            remove(&mut resolved.carry_input, keys::PRIMARY);
        }
        resolved
    }
}
