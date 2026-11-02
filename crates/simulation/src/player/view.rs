//! Source lean, independent look, zoom, and head tracking.
use crate::{
    player::{
        PlayerStepContext,
        head_bob::{CarHeadBob, WalkBob},
    },
    reference::{PlayerPoseReference, PlayerReference, PlayerSettings, PlayerViewReference},
    simulation::{FrameInput, keys, player_keys},
    vehicle::VehiclePose,
};
use rapier3d::math::{Pose, Rotation, Vector};

#[derive(Clone, Copy, Debug)]
pub struct PlayerView {
    pub head_pose: VehiclePose,
    pub camera_pose: VehiclePose,
    pub fov_degrees: f32,
    pub ray_actions_blocked: bool,
}

pub(super) struct ViewState {
    reference: PlayerViewReference,
    standing: Vector,
    prone: Vector,
    pub(super) settings: PlayerSettings,
    charge: Option<f32>,
    independent: [f32; 2],
    camera_rotation: Rotation,
    seat_entry_rotation: Option<Rotation>,
    return_remaining: f32,
    standing_lean: bool,
    seated_lean: bool,
    looking: bool,
    lean: Vector,
    head: Vector,
    height: f32,
    zoom: f32,
    walk_bob: WalkBob,
    car_bob: CarHeadBob,
    car_offset: Vector,
}
impl ViewState {
    pub(super) fn new(
        player: &PlayerReference,
        pose: &PlayerPoseReference,
        settings: PlayerSettings,
    ) -> Self {
        let posture = &player.posture;
        let standing = Vector::from_array(posture.standing_head.translation);
        Self {
            reference: player.view.clone(),
            standing,
            prone: Vector::from_array(posture.prone_head.translation),
            settings,
            charge: None,
            independent: [0.; 2],
            camera_rotation: Rotation::IDENTITY,
            seat_entry_rotation: None,
            return_remaining: 0.,
            standing_lean: false,
            seated_lean: false,
            looking: false,
            lean: standing,
            head: standing,
            height: 0.,
            zoom: 1.,
            walk_bob: WalkBob::new(player),
            car_bob: CarHeadBob::new(pose),
            car_offset: Vector::ZERO,
        }
    }
    fn neutral_head(&self, posture: f32) -> Vector {
        self.prone
            .lerp(self.standing + Vector::Y * self.walk_bob_offset(), posture)
    }
    /// Source `TStandHeadPos`: the bob adds only with `BWalkHeadBob`. It still runs for footsteps.
    fn walk_bob_offset(&self) -> f32 {
        if self.settings.head_bob[0] {
            self.walk_bob.offset
        } else {
            0.
        }
    }
    pub(super) fn step_walk_bob(&mut self, active: bool, speed: f32, dt: f32) {
        self.car_bob.reset();
        self.car_offset = Vector::ZERO;
        self.walk_bob.step(active, speed, dt);
    }
    pub(super) fn step_car_bob(&mut self, dt: f32, velocity: Vector, dead: bool, head: Rotation) {
        self.walk_bob.beat = false;
        self.walk_bob.add = 0.;
        // Source `BDriveHeadBob` off: the head stays at the lean position and the bob stops.
        if self.settings.head_bob[1] {
            self.car_offset = self.car_bob.step(dt, velocity, dead, head);
        } else {
            self.car_offset = Vector::ZERO;
        }
    }
    pub(super) fn footstep_beat(&self) -> bool {
        self.walk_bob.beat
    }
    pub(super) fn walk_add(&self) -> f32 {
        self.walk_bob.add
    }
    pub(super) fn source_anchor_midpoint(&self, posture: f32, seated: bool) -> Vector {
        let direction_anchor = if seated {
            self.lean
        } else {
            self.neutral_head(posture)
        };
        (direction_anchor + self.prone) * 0.5
    }
    pub(super) fn cancel(&mut self, posture: f32) {
        self.charge = None;
        self.independent = [0.; 2];
        self.camera_rotation = Rotation::IDENTITY;
        self.seat_entry_rotation = None;
        self.return_remaining = 0.;
        self.standing_lean = false;
        self.seated_lean = false;
        self.looking = false;
        self.height = 0.;
        self.zoom = 1.;
        self.lean = self.neutral_head(posture);
        self.head = self.lean;
    }
    /// Source `GetLook` `BkeyLook`: seated, not leaning and with key look held, the move keys look.
    pub(super) fn key_look(&self, input: &FrameInput, seated: bool) -> Option<[f32; 2]> {
        if !seated || self.seated_lean || input.player_held & player_keys::KEY_LOOK == 0 {
            return None;
        }
        let key = |bit: u32| (input.held & bit != 0) as u8 as f32;
        let scale = self.settings.key_look;
        // Browser +Y is down. Source LOOKY is positive up.
        Some([
            (key(keys::RIGHT) - key(keys::LEFT)) * scale,
            (key(keys::BACK) - key(keys::FORWARD)) * scale,
        ])
    }
    /// Returns the ordinary look multiplier when this mouse sample is available.
    pub(super) fn step(
        &mut self,
        input: FrameInput,
        context: PlayerStepContext,
        posture: f32,
        seated: bool,
        look_mode: bool,
        head_rotation: Rotation,
        dt: f32,
    ) -> Option<f32> {
        let zooming = input.player_held & player_keys::ZOOM != 0
            || (input.held & keys::TRANSLATE != 0 && !context.picked_up);
        let (target, rate) = if zooming {
            (self.reference.zoom_multiplier, self.reference.zoom_in_rate)
        } else {
            (1., self.reference.zoom_out_rate)
        };
        self.zoom += (target - self.zoom) * (dt * rate).clamp(0., 1.);
        let head_rotation = self.seat_entry_rotation.take().unwrap_or(head_rotation);
        let held = input.player_held & player_keys::LEAN != 0;
        if input.player_pressed & player_keys::LEAN != 0 {
            self.charge = Some(0.);
        }
        if input.player_released & player_keys::LEAN != 0 {
            if !seated
                && self.charge.is_some_and(|charge| {
                    super::source_short_hold(charge, self.reference.tap_seconds)
                })
            {
                self.standing_lean = !self.standing_lean;
            }
            self.charge = None;
        }
        if held && let Some(charge) = &mut self.charge {
            *charge += dt;
        }
        let mouse = input.mouse.map(|v| if v.is_finite() { v } else { 0. });
        let scroll = if input.wheel.is_finite() {
            input.wheel * self.settings.player_scroll()
        } else {
            0.
        };
        // Source lookInsteadOfLean: a seated lean turns the camera, and release lerps it back.
        let look_mode = seated && look_mode;
        self.seated_lean = seated && held && !look_mode;
        self.looking = (!seated || look_mode) && (held || self.standing_lean);
        if look_mode {
            if held {
                self.turn_independent(mouse, context.mouse_consumed);
            } else {
                let amount = (dt * self.reference.return_rate).min(1.);
                for angle in &mut self.independent {
                    *angle -= *angle * amount;
                }
                self.set_camera_rotation();
            }
        } else if seated {
            self.independent = [0.; 2];
            self.camera_rotation = Rotation::IDENTITY;
            self.height = 0.;
        } else {
            if context.duck_allowed && scroll > 0. && posture > 0.95 {
                self.height = (self.height + scroll * self.reference.height_scroll_step).clamp(
                    self.reference.height_limits[0],
                    self.reference.height_limits[1],
                );
            }
            if held {
                self.turn_independent(mouse, context.mouse_consumed);
                self.return_remaining = self.reference.return_window_seconds;
            } else {
                self.independent = [0.; 2];
                self.return_remaining = (self.return_remaining - dt).max(0.);
                self.camera_rotation = if self.return_remaining > 0. {
                    self.camera_rotation.lerp(
                        Rotation::IDENTITY,
                        (dt * self.reference.return_rate).min(1.),
                    )
                } else {
                    Rotation::IDENTITY
                };
                self.height = 0.;
            }
        }
        let leaning = self.seated_lean || (!seated && self.standing_lean && !held);
        if leaning {
            let forward = if input.held & keys::RUN != 0 {
                Vector::Y
            } else {
                let forward = head_rotation * -Vector::Z;
                Vector::new(forward.x, 0., forward.z).normalize_or_zero()
            };
            // Browser +Y is down. Source LOOKY is positive up.
            self.lean += (forward * -mouse[1]
                + head_rotation * Vector::X * mouse[0]
                + Vector::Y * scroll * self.reference.lean_scroll_multiplier)
                * dt
                * self.reference.lean_sensitivity;
        } else if !seated && !self.standing_lean {
            self.lean = self.lean.lerp(
                self.neutral_head(posture)
                    + if held {
                        Vector::Y * self.height
                    } else {
                        Vector::ZERO
                    },
                (dt * self.reference.return_rate).min(1.),
            );
        }
        if self.standing_lean {
            let center = (self.standing + self.prone + self.neutral_head(posture)) * 0.3333;
            self.lean =
                center + (self.lean - center).clamp_length_max(self.reference.lean_distance);
        } else if seated {
            self.lean = self.standing
                + (self.lean - self.standing).clamp_length_max(self.reference.lean_distance);
        }
        self.head = self
            .head
            .lerp(self.lean, (dt * self.reference.head_lerp).min(1.));
        (!held && !(self.standing_lean && !seated) && !context.mouse_consumed).then_some(
            if context.binoculars {
                crate::interaction::binoculars::LOOK_SCALE
            } else if zooming {
                self.reference.zoom_multiplier
            } else {
                1.
            },
        )
    }
    fn turn_independent(&mut self, mouse: [f32; 2], consumed: bool) {
        let limit = self.reference.free_look_limit_degrees;
        for (angle, delta) in
            self.independent
                .iter_mut()
                .zip(if consumed { [0.; 2] } else { mouse })
        {
            *angle = (*angle + delta).clamp(-limit, limit);
        }
        self.set_camera_rotation();
    }
    fn set_camera_rotation(&mut self) {
        self.camera_rotation = Rotation::from_rotation_y(-self.independent[0].to_radians())
            * Rotation::from_rotation_x(-self.independent[1].to_radians());
    }
    pub(super) fn enter(&mut self, head_in_seat: Pose) {
        self.cancel(1.);
        self.car_bob.reset();
        self.car_offset = Vector::ZERO;
        self.head = head_in_seat.translation;
        self.seat_entry_rotation = Some(head_in_seat.rotation);
    }
    /// Seat entry head rotation, and the camera rotation after the head.
    pub(super) fn look_rotations(&self) -> (Option<Rotation>, Rotation) {
        (self.seat_entry_rotation, self.camera_rotation)
    }
    pub(super) fn looking(&self) -> bool {
        self.looking
    }
    pub(super) fn recenter(&mut self) {
        self.lean = self.standing;
    }
    pub(super) fn resolve(
        &self,
        body: Pose,
        head_rotation: Rotation,
        seated: bool,
        neutral_posture: Option<f32>,
    ) -> PlayerView {
        let head = Pose::from_parts(
            body.translation
                + body.rotation * neutral_posture.map_or(self.head, |p| self.neutral_head(p))
                + if neutral_posture.is_none() {
                    self.car_offset
                } else {
                    Vector::ZERO
                },
            body.rotation
                * if neutral_posture.is_none() {
                    self.seat_entry_rotation.unwrap_or(head_rotation)
                } else {
                    head_rotation
                },
        );
        let camera = Pose::from_parts(
            head.translation,
            head.rotation
                * if neutral_posture.is_some() {
                    Rotation::IDENTITY
                } else {
                    self.camera_rotation
                },
        );
        PlayerView {
            head_pose: super::from_rapier(head),
            camera_pose: super::from_rapier(camera),
            fov_degrees: self.settings.fov[usize::from(seated)]
                * if neutral_posture.is_some() {
                    1.
                } else {
                    self.zoom
                },
            ray_actions_blocked: neutral_posture.is_none() && self.seated_lean,
        }
    }
}
pub(super) fn exit_permission(camera: Pose, seat: Pose, minimum_angle: f32) -> bool {
    let angle = (camera.rotation * -Vector::Z)
        .normalize()
        .dot((seat.rotation * -Vector::Z).normalize())
        .clamp(-1., 1.)
        .acos()
        .to_degrees();
    angle + 1.0e-5 >= minimum_angle
}

/// Permission uses the camera. This source placement search uses only the head.
pub(super) fn search_exit(
    head: Pose,
    limits: &super::ExitReference,
    padding: f32,
    capsule: &dyn rapier3d::parry::shape::Shape,
    rays: &rapier3d::prelude::QueryPipeline<'_>,
    clearance: &rapier3d::prelude::QueryPipeline<'_>,
) -> Option<Pose> {
    use rapier3d::prelude::Ray;
    let forward = head.rotation * -Vector::Z;
    let max_distance = rays
        .cast_ray_and_get_normal(&Ray::new(head.translation, forward), limits.distance, true)
        .map_or(limits.distance, |(_, hit)| {
            let displacement = forward * hit.time_of_impact + hit.normal * padding;
            Vector::new(displacement.x, 0., displacement.z)
                .length()
                .min(limits.distance)
        });
    let mut vertical = 0.;
    for index in 0..limits.horizontal_steps {
        let distance = index as f32 * limits.horizontal_step;
        if distance >= max_distance {
            break;
        }
        for _ in 0..limits.vertical_steps {
            let point = head.translation + forward * distance + Vector::Y * vertical;
            if clearance
                .intersect_shape(Pose::from_translation(point), capsule)
                .next()
                .is_none()
            {
                let mut candidate = Pose::from_translation(point);
                candidate.rotation = Rotation::from_rotation_y(
                    -super::yaw_from_rotation(head.rotation).to_radians(),
                );
                return Some(candidate);
            }
            vertical += limits.vertical_step;
        }
    }
    None
}
