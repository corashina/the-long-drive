//! Source `fedospawnscript` on the Player and the `fedoscript` UFO that it spawns.
//! The UFO is a dynamic body without gravity and with frozen rotation. The AI follows the road
//! above the terrain and chases the player. A seated player flies it. Saves do not keep it.
//! Source +Z is port -Z.
use crate::creatures::stimuli::Stimulus;
use crate::interaction::InteractionState;
use crate::interaction::controls::UseControl;
use crate::interaction::items::ItemStore;
use crate::lighting::CyclePhase;
use crate::player::SeatOwner;
use crate::player::body::SeatFrames;
use crate::reference::{
    CreaturesReference, GroundUfoReference, ItemId, ReferenceError, SkyReference, StimulusSource,
    UfoRole,
};
use crate::simulation::dials::{inverse_lerp, lerp};
use crate::sky_traffic::Plane;
use crate::vehicle::{PhysicsWorld, VehiclePose, collider_builder};
use crate::weather::StormRandom;
use crate::world::WorldOrigin;
use crate::world::stream::content_seats::{ItemSeat, WorldSeatBodyPose, WorldSeatReference};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{
    ColliderHandle, Group, InteractionGroups, InteractionTestMode, RigidBodyBuilder,
    RigidBodyHandle,
};
use std::sync::Arc;

/// Ground UFO plane ids start here, above the sky plane ids.
const ID_BASE: u32 = 1 << 31;
/// Source `OneCoord(transform.position + transform.forward * 50f)`.
const LOOK_AHEAD: f64 = 50.;
/// Source `PlayerInput`: the target distance of each input.
const INPUT_DISTANCE: f32 = 100.;
/// Source `roadHeightOffset += setAltitude.tunedFloat * 20f`.
const ALTITUDE_STEP: f32 = 20.;
/// Unity default material.
const FRICTION: f32 = 0.6;

/// A fedoscript usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UfoControl {
    /// `enableFollowRoad`: turn.
    AutoPilot,
    /// `setHighSpeed`: tune.
    Speed,
    /// `setAltitude`: tune.
    Altitude,
}

/// A usable collider of one UFO. `item` names the UFO in interaction targets. It has no item
/// state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UfoControlTarget {
    pub ufo: u32,
    pub item: ItemId,
    pub control: UfoControl,
}

/// Source `inputscript` values that `PlayerInput` reads. A positive port steer turns left.
#[derive(Clone, Copy, Debug, Default)]
pub struct UfoPilot {
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    /// `Input.GetButton("jump")`.
    pub up: bool,
    /// `Input.GetButton("duck")`.
    pub down: bool,
}

/// The world the UFOs live in.
pub struct UfoWorld<'a> {
    pub physics: &'a mut PhysicsWorld,
    pub interaction: &'a mut InteractionState,
    pub items: &'a mut ItemStore,
    pub origin: &'a WorldOrigin,
}

struct UfoCollider {
    handle: ColliderHandle,
    /// Pose on the body without the rotor spin.
    base: Pose,
    spin: bool,
    seated: bool,
}

pub struct Ufo {
    pub id: u32,
    pub body: RigidBodyHandle,
    colliders: Vec<UfoCollider>,
    /// Source `targetObj`, absolute.
    target: [f64; 3],
    current_speed: f32,
    visited: bool,
    last_player: [f64; 3],
    ai: bool,
    follow_road: bool,
    speed_lerp: f32,
    road_height_offset: f32,
    /// `seat.inUse` of the last step.
    in_use: bool,
    /// `enableFollowRoad.turned`.
    turned: bool,
    /// `setHighSpeed.tunedFloat` while tuned.
    speed_tune: Option<f32>,
    /// `setAltitude.tunedFloat` once tuned. Source `Update` resets `setHighSpeed` after the
    /// altitude change, so the altitude tune stays set.
    altitude_tune: Option<f32>,
}

impl Ufo {
    /// Source `fedoscript.Update` and `Go`. Returns false when the source destroys the UFO.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        r: &GroundUfoReference,
        player: [f64; 3],
        pilot: Option<UfoPilot>,
        dt: f32,
        spin: Rotation,
        world: &mut UfoWorld,
        road: &impl Fn([f64; 3], f32) -> [f64; 3],
    ) -> bool {
        let position = world.origin.absolute(
            world.physics.rapier().bodies[self.body]
                .translation()
                .to_array(),
        );
        let in_use = pilot.is_some();
        if self.ai {
            // The source destroys at the end of the frame. A seated player keeps the UFO.
            if !self.ai_input(r, position, player, dt) && !in_use {
                return false;
            }
            if in_use {
                self.ai = false;
                self.follow_road = false;
            }
        } else {
            if let Some(pilot) = pilot {
                if std::mem::take(&mut self.turned) {
                    self.follow_road = !self.follow_road;
                }
                if let Some(tune) = self.speed_tune.take() {
                    self.speed_lerp = (self.speed_lerp + tune).clamp(0., 1.);
                }
                if let Some(tune) = self.altitude_tune {
                    self.road_height_offset += tune * ALTITUDE_STEP;
                }
                self.player_input(position, pilot);
            }
            self.current_speed = lerp(r.speed, r.high_speed, self.speed_lerp);
        }
        if in_use != self.in_use {
            self.in_use = in_use;
            for c in self.colliders.iter().filter(|c| c.seated) {
                world.physics.edit_collider(c.handle, |c| {
                    crate::vehicle::set_collider_enabled(c, in_use)
                });
            }
        }
        if self.follow_road {
            let mut ahead = position;
            ahead[2] -= LOOK_AHEAD;
            self.target = road(ahead, self.road_height_offset);
        }
        // Go.
        self.current_speed = self.current_speed.clamp(r.speed, r.high_speed);
        let delta: [f64; 3] = std::array::from_fn(|i| self.target[i] - position[i]);
        let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        let velocity = if length < f64::from(r.target_distance) || length == 0. {
            Vector::ZERO
        } else {
            Vector::from_array(std::array::from_fn(|i| (delta[i] / length) as f32))
                * self.current_speed
        };
        world
            .physics
            .edit_body(self.body, |b| b.set_linvel(velocity, true));
        // rot.Rotate. The rotor colliders turn with the rotor.
        for c in self.colliders.iter().filter(|c| c.spin) {
            let pose = Pose::from_rotation(spin) * c.base;
            world
                .physics
                .edit_collider(c.handle, |c| c.set_position_wrt_parent(pose));
        }
        true
    }

    /// Source `AiInput`. Returns false when the source destroys the UFO.
    fn ai_input(
        &mut self,
        r: &GroundUfoReference,
        position: [f64; 3],
        player: [f64; 3],
        dt: f32,
    ) -> bool {
        let dist = distance(position, player) as f32;
        let visited_distance = r.visited_distance + r.road_height_offset;
        if dist < visited_distance {
            self.visited = true;
        }
        let target_speed = distance(player, self.last_player) as f32 / dt;
        let multiplier = lerp(
            r.ai_multiplier_range[0],
            r.ai_multiplier_range[1],
            inverse_lerp(r.ai_distance_range[0], r.ai_distance_range[1], dist),
        );
        let mut alive = true;
        if self.visited {
            if dist > visited_distance {
                self.current_speed = self.current_speed.max(target_speed * multiplier);
            } else {
                self.current_speed = target_speed * multiplier;
            }
            alive = dist <= r.disappear_distance;
        } else {
            self.current_speed = target_speed.clamp(r.speed, r.high_speed) * multiplier;
        }
        self.last_player = player;
        alive
    }

    /// Source `PlayerInput`. The rotation is identity, because source forward is port -Z.
    fn player_input(&mut self, position: [f64; 3], p: UfoPilot) {
        let d = f64::from(INPUT_DISTANCE);
        let mut target = position;
        target[2] -= f64::from(p.throttle) * d;
        target[0] -= f64::from(p.steer) * d;
        target[2] += f64::from(p.brake) * d;
        if p.up {
            target = position;
            target[1] += d;
        }
        if p.down {
            target = position;
            target[1] -= d;
        }
        self.target = target;
    }
}

pub struct GroundUfos {
    random: StormRandom,
    current_night: i32,
    selected_night: i32,
    added: bool,
    next_id: u32,
    ufos: Vec<Ufo>,
    /// The UFOs as sky planes, relative to the sky anchor.
    planes: Vec<Plane>,
    stimuli: Vec<Stimulus>,
    seat: Arc<WorldSeatReference>,
    stimulus: Option<StimulusSource>,
}

impl GroundUfos {
    /// Source `Start`: `selectedNight = Random.Range(minNight, maxNight + 1)`.
    pub fn new(r: &GroundUfoReference, creatures: &CreaturesReference, seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x6675_666f_0000_0001);
        let selected_night = night(&mut random, r);
        Self {
            random,
            current_night: 0,
            selected_night,
            added: false,
            next_id: ID_BASE,
            ufos: Vec::new(),
            planes: Vec::new(),
            stimuli: Vec::new(),
            seat: Arc::new(seat_reference(r)),
            stimulus: creatures.stimulus(&r.stimulus.source).cloned(),
        }
    }
    pub fn ufos(&self) -> &[Ufo] {
        &self.ufos
    }
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }
    /// The stimulscript of each UFO.
    pub fn stimuli(&self) -> &[Stimulus] {
        &self.stimuli
    }

    /// Source `usablescript.Turn` (value 0) or a tune of `tunedFloat` on a UFO usable.
    pub fn use_control(&mut self, target: UfoControlTarget, value: f32) {
        let Some(ufo) = self.ufos.iter_mut().find(|u| u.id == target.ufo) else {
            return;
        };
        match target.control {
            UfoControl::AutoPilot => ufo.turned = true,
            UfoControl::Speed if value != 0. => ufo.speed_tune = Some(value),
            UfoControl::Altitude if value != 0. => ufo.altitude_tune = Some(value),
            _ => {}
        }
    }

    /// Source `fedospawnscript.Update` and each `fedoscript.Update`. Run before the physics
    /// step. `seated` is the UFO of the player seat and the pilot input. `spawns` is false on a
    /// guest, because the source spawns only on the server. `road` gives the road point under an
    /// absolute position at a height above the base terrain. `now` turns the rotor colliders.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        r: &SkyReference,
        phase: CyclePhase,
        player: [f64; 3],
        seated: Option<(u32, UfoPilot)>,
        spawns: bool,
        now: f64,
        dt: f32,
        world: &mut UfoWorld,
        road: impl Fn([f64; 3], f32) -> [f64; 3],
    ) -> Result<(), ReferenceError> {
        let g = &r.ground_ufo;
        let spin = r.prefabs[g.prefab]
            .spin
            .as_ref()
            .map_or(Rotation::IDENTITY, |s| {
                let axis = Vector::from_array(s.axis);
                let degrees = (f64::from(s.degrees_per_second * axis.length()) * now) % 360.;
                Rotation::from_axis_angle(axis.normalize(), (degrees as f32).to_radians())
            });
        // A new UFO first moves in the next frame.
        let mut i = 0;
        while i < self.ufos.len() {
            let ufo = &mut self.ufos[i];
            let pilot = seated.filter(|(id, _)| *id == ufo.id).map(|(_, p)| p);
            if ufo.step(g, player, pilot, dt, spin, world, &road) {
                i += 1;
            } else {
                let ufo = self.ufos.swap_remove(i);
                for c in &ufo.colliders {
                    world.interaction.controls.remove(c.handle);
                }
                world.physics.remove_body(ufo.body);
            }
        }
        match phase {
            CyclePhase::Dawn => self.added = false,
            CyclePhase::Night if !self.added => {
                self.added = true;
                self.current_night += 1;
                if self.current_night >= self.selected_night {
                    self.selected_night = self.current_night + night(&mut self.random, g);
                    if self.random.unit() < g.chance {
                        if spawns {
                            self.spawn(g, player, world)?;
                        }
                        self.selected_night += g.nights_after_spawn;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Refreshes `planes` for the sky anchor and the stimuli after the physics step.
    pub fn place(&mut self, r: &GroundUfoReference, anchor: [f64; 3], world: &UfoWorld) {
        self.planes.clear();
        self.stimuli.clear();
        for ufo in &self.ufos {
            let local = world.physics.rapier().bodies[ufo.body].translation();
            let position = world.origin.absolute(local.to_array());
            self.planes.push(Plane {
                id: ufo.id,
                prefab: r.prefab,
                offset: Vector::from_array(std::array::from_fn(|i| {
                    (position[i] - anchor[i]) as f32
                })),
                heading: Rotation::IDENTITY,
                contrail: None,
                seated: ufo.in_use,
            });
            if let Some(s) = &self.stimulus {
                self.stimuli.extend(s.stimuli.iter().map(|v| Stimulus {
                    position: local + Vector::from_array(r.stimulus.center),
                    kind: v.kind,
                    value: v.value,
                    visible: s.visible,
                    sound: s.sound,
                    radius: r.stimulus.radius,
                    body: Some(ufo.body),
                }));
            }
        }
    }

    /// Source `DoSpawn` and `fedoscript.Spawned`.
    fn spawn(
        &mut self,
        r: &GroundUfoReference,
        player: [f64; 3],
        world: &mut UfoWorld,
    ) -> Result<(), ReferenceError> {
        let side = self.random.range(-r.side_distance, r.side_distance);
        let back = self.random.range(r.back_range[0], r.back_range[1]);
        // Source -Z back is port +Z.
        let position = [
            player[0] + f64::from(side),
            player[1] + f64::from(r.up_distance),
            player[2] + f64::from(back),
        ];
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(ID_BASE);
        let item = world.items.allocate_item_id()?;
        let body = world.physics.insert_body(
            RigidBodyBuilder::dynamic()
                .translation(Vector::from_array(world.origin.local(position)))
                .gravity_scale(0.)
                .lock_rotations()
                .ccd_enabled(r.ccd)
                .additional_mass(r.mass),
        );
        let mut colliders = Vec::with_capacity(r.colliders.len());
        for c in &r.colliders {
            let builder = collider_builder(&c.geometry)?;
            let base = builder.position;
            let collider = builder
                .friction(FRICTION)
                .density(0.)
                .enabled(!c.seated)
                .solver_groups(crate::vehicle::solver_groups(!c.seated))
                .collision_groups(InteractionGroups::new(
                    Group::from_bits_truncate(c.membership),
                    Group::from_bits_truncate(c.filter),
                    InteractionTestMode::And,
                ))
                .build();
            let handle = world.physics.insert_collider_with_parent(collider, body);
            colliders.push(UfoCollider {
                handle,
                base,
                spin: c.spin,
                seated: c.seated,
            });
            let control = match c.role {
                UfoRole::Body => None,
                UfoRole::Seat => {
                    let seat = self.item_seat(r, id, item, body);
                    world
                        .interaction
                        .controls
                        .bind(handle, UseControl::Seat(seat));
                    None
                }
                UfoRole::AutoPilot => Some(UfoControl::AutoPilot),
                UfoRole::Speed => Some(UfoControl::Speed),
                UfoRole::Altitude => Some(UfoControl::Altitude),
            };
            if let Some(control) = control {
                let target = UfoControlTarget {
                    ufo: id,
                    item,
                    control,
                };
                world
                    .interaction
                    .controls
                    .bind(handle, UseControl::Ufo(target));
            }
        }
        world.physics.refresh_queries();
        self.ufos.push(Ufo {
            id,
            body,
            colliders,
            target: position,
            current_speed: 0.,
            visited: false,
            last_player: player,
            ai: true,
            follow_road: true,
            speed_lerp: r.speed_lerp,
            road_height_offset: r.road_height_offset,
            in_use: false,
            turned: false,
            speed_tune: None,
            altitude_tune: None,
        });
        Ok(())
    }

    fn item_seat(
        &self,
        r: &GroundUfoReference,
        id: u32,
        item: ItemId,
        body: RigidBodyHandle,
    ) -> Arc<ItemSeat> {
        let at = |position: [f32; 3]| VehiclePose {
            position,
            rotation: [0., 0., 0., 1.],
        };
        Arc::new(ItemSeat {
            item,
            seat_item: item,
            owner: SeatOwner::Ufo(id),
            body,
            pose: at(r.seat.sit),
            node_pose: at(r.seat.node),
            seat: self.seat.clone(),
            frames: SeatFrames {
                def_pose: r.seat.def_pose as usize,
                driving: false,
                hands: [None; 2],
                legs: [None; 2],
                driving_hands: [None; 2],
                driving_legs: [None; 2],
            },
        })
    }
}

/// The seatscript as a world seat. It has no prototype nodes and no limb frames.
fn seat_reference(r: &GroundUfoReference) -> WorldSeatReference {
    let s = &r.seat;
    WorldSeatReference {
        source: s.source.clone(),
        sit_node: 0,
        hit_nodes: Vec::new(),
        yaw_limits_degrees: [-s.rot_limit, s.rot_limit],
        yaw_unlimited: s.yaw_unlimited,
        cant_sit_angle_degrees: None,
        look_instead_of_lean: s.look_instead_of_lean,
        root_origin: s.root_origin,
        pitch_limits_degrees: [-89., 89.],
        default_pitch_degrees: s.default_pitch,
        min_exit_angle_degrees: s.min_exit_angle,
        exit_height: s.exit_height,
        show_sleep_text: s.show_sleep_text,
        cant_push_from_seat: s.cant_push_from_seat,
        push_force_modifier: s.push_force_modifier,
        driver: false,
        driving2: false,
        body_pose: WorldSeatBodyPose {
            def_pose: s.def_pose,
            left_hand: None,
            right_hand: None,
            left_leg: None,
            right_leg: None,
        },
    }
}

/// `Random.Range(minNight, maxNight + 1)`.
fn night(random: &mut StormRandom, r: &GroundUfoReference) -> i32 {
    let count = r.night_range[1] + 1 - r.night_range[0];
    r.night_range[0] + ((random.unit() * count as f32) as i32).min(count - 1)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}
