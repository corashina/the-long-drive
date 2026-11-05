//! Persistent imported home bodies and preparation state.
pub mod recovery;
mod under_map;
use crate::car::{CarInput, CarWorld, VehicleRuntime, car_sound, instrument_sample, world_vehicle};
use crate::interaction::{
    InteractionAction, InteractionState, ResolvedAim,
    items::{ItemStore, node_pose},
};
use crate::player::{PlayerState, PlayerStepContext};
use crate::reference::{BodyId, ItemId, NodeId, PrototypeTool, ReferenceError, ReferenceManifest};
use crate::simulation::{
    FIXED_DT, FrameInput,
    engine::{EngineInput, EngineOutput, EngineRecords},
    keys,
    powertrain::PowertrainParts,
};
use crate::vehicle::{PhysicsWorld, VehiclePose, collider_builder, to_rapier};
use crate::world::{DEFAULT_SEED, ObjectKey, WorldStream};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::{
    ActiveEvents, Group, InteractionGroups, InteractionTestMode, QueryFilter, RigidBodyBuilder,
    RigidBodyHandle, Vector,
};
use std::collections::{BTreeMap, HashMap};

mod audio_frame;
mod body;
pub mod cigarettes;
mod computers;
mod electronics;
mod explosions;
mod frame;
mod frame_particles;
mod guns;
mod home_cable;
pub mod hose;
mod hud;
mod hum;
mod humans;
mod menu_view;
mod population;
mod rabbits;
mod ragdoll;
mod reach;
pub mod spawned;
pub mod structure_frame;
mod survival;
pub mod wear;
mod weather;
mod world_step;
pub use explosions::explosive_imported;
pub use survival::DrinkResult;
pub use survival::active_part as active_food_part;
pub use survival::saved_food;
pub use world_step::WorldInput;

pub struct StartingArea {
    pub bodies: HashMap<BodyId, RigidBodyHandle>,
    pub items: ItemStore,
    pub interaction: InteractionState,
    pub inventory: crate::inventory::InventoryState,
    pub pose: crate::player::pose::PlayerPoseState,
    pub body: crate::player::body::BodyState,
    pub ragdoll: crate::player::ragdoll::Ragdoll,
    inventory_offsets: [Option<(ItemId, VehiclePose)>; 4],
    inventory_clutch_conflict: bool,
    pub player: PlayerState,
    pub origin: [f64; 3],
    attachments: Vec<(RigidBodyHandle, RigidBodyHandle, rapier3d::math::Pose)>,
    pub render_poses: Vec<(BodyId, VehiclePose, VehiclePose)>,
    pub item_render_poses: Vec<(ItemId, VehiclePose, VehiclePose)>,
    pub spawned: spawned::SpawnedItems,
    /// Runtime food copies: turds and cut pieces.
    pub foods: Vec<ItemId>,
    /// gibscript and scaleDestroyScript of spawned gibs.
    pub gibs: crate::damage::gibs::Gibs,
    pub cigarettes: cigarettes::CigaretteRuntime,
    pub wear: wear::WearRuntime,
    portable_mass: Vec<(RigidBodyHandle, f32, Vec<ItemId>, f32)>,
}

impl StartingArea {
    pub fn new(
        reference: &ReferenceManifest,
        physics: &mut PhysicsWorld,
        origin: [f64; 3],
    ) -> Result<Self, ReferenceError> {
        let mut items = ItemStore::new(reference)?;
        let mut interaction = InteractionState::new(reference)?;
        let mut bodies = HashMap::with_capacity(reference.bodies.len());
        for body in &reference.bodies {
            let pose = VehiclePose {
                position: std::array::from_fn(|i| body.pose.translation[i] - origin[i] as f32),
                rotation: body.pose.rotation,
            };
            let builder = if body.parent_body.is_some() {
                RigidBodyBuilder::kinematic_position_based()
            } else if body.dynamic {
                RigidBodyBuilder::dynamic()
                    .ccd_enabled(true)
                    .additional_mass(body.mass_kg)
                    .linear_damping(body.linear_damping)
                    .angular_damping(body.angular_damping)
            } else {
                RigidBodyBuilder::fixed()
            };
            let inactive_flashlight = reference
                .flashlights
                .instances
                .iter()
                .any(|instance| instance.node == body.node && !instance.startup_active);
            let handle =
                physics.insert_body(builder.enabled(!inactive_flashlight).pose(to_rapier(pose)));
            bodies.insert(body.id, handle);
        }
        for record in &reference.colliders {
            let item = reference
                .action_bindings
                .iter()
                .find(|binding| binding.collider == record.id)
                .and_then(|binding| binding.use_owner)
                .or_else(|| collider_item(reference, record.node));
            let hit_sound = item.is_some_and(|id| {
                reference
                    .items
                    .iter()
                    .any(|i| i.id == id && i.collision_clip.is_some())
            });
            let builder = collider_builder(&record.geometry)?
                .friction(record.friction)
                .restitution(record.restitution)
                .sensor(record.sensor)
                .enabled(record.enabled)
                .active_events(if hit_sound {
                    ActiveEvents::COLLISION_EVENTS
                } else {
                    ActiveEvents::empty()
                })
                .solver_groups(crate::vehicle::solver_groups(record.enabled))
                .density(0.0)
                .collision_groups(InteractionGroups::new(
                    Group::from_bits_truncate(record.membership),
                    Group::from_bits_truncate(record.filter),
                    InteractionTestMode::And,
                ))
                .user_data(
                    ((record.id.0 as u128) << 64)
                        | crate::vehicle::step_surface_bits(record.step_surface)
                        | crate::vehicle::tag_bits(record.tag)
                        | item.map_or(0, |item| item.0 as u128 + 1),
                );
            physics.insert_collider_with_parent(builder.build(), bodies[&record.body]);
        }
        for (&id, &handle) in &bodies {
            items.bind_body(id, handle);
            interaction.bind_body(id, handle);
        }
        for (handle, collider) in physics.rapier().colliders.iter() {
            let id = crate::reference::ColliderId((collider.user_data >> 64) as u32);
            if id.0 != 0 {
                interaction.bind_collider(id, handle);
            }
        }
        interaction.rebase(origin.map(|v| v as f32));
        interaction.sync_mechanisms(physics);
        interaction.parts.bind(&mut items, physics)?;
        interaction.sync_parts(&mut items, physics)?;
        let starter = reference.survival.starter_food;
        if let Some(food) = items.get(starter).and_then(|item| item.food) {
            let body = items
                .get(starter)
                .and_then(|item| item.body)
                .ok_or_else(|| ReferenceError("Starter food body missing".into()))?;
            for &handle in physics.rapier().bodies[body].colliders().to_vec().iter() {
                let collider_id = (physics.rapier().colliders[handle].user_data >> 64) as u32;
                if let Some(record) = reference
                    .colliders
                    .iter()
                    .find(|entry| entry.id.0 == collider_id)
                {
                    let enabled = record.enabled
                        && survival::active_part(
                            reference,
                            record.node,
                            &reference.survival.starter_parts,
                            food.bite,
                        );
                    physics.edit_collider(handle, |collider| collider.set_enabled(enabled));
                }
            }
        }
        physics.refresh_queries();
        let dropping: Vec<_> = reference
            .bodies
            .iter()
            .filter(|body| body.dynamic && body.parent_body.is_none())
            .map(|body| bodies[&body.id])
            .collect();
        rest_on_support(physics, &dropping);
        items.sync_poses(physics);
        let spawn = &reference.area.player_spawn;
        let player = PlayerState::new(
            &reference.player,
            &reference.clothing.pose,
            crate::reference::PlayerSettings::defaults(reference),
            VehiclePose {
                position: std::array::from_fn(|i| spawn.translation[i] - origin[i] as f32),
                rotation: spawn.rotation,
            },
            physics,
        );
        interaction.bind_player_body(player.body_handle());
        let attachments = reference
            .bodies
            .iter()
            .filter_map(|record| {
                let parent = record.parent_body?;
                if reference
                    .mechanisms
                    .iter()
                    .any(|m| m.driven_nodes.contains(&record.node))
                {
                    return None;
                }
                let parent_record = reference
                    .bodies
                    .iter()
                    .find(|body| body.id == parent)
                    .expect("validated parent");
                let source = to_rapier(VehiclePose {
                    position: record.pose.translation,
                    rotation: record.pose.rotation,
                });
                let parent_source = to_rapier(VehiclePose {
                    position: parent_record.pose.translation,
                    rotation: parent_record.pose.rotation,
                });
                Some((
                    bodies[&record.id],
                    bodies[&parent],
                    parent_source.inverse() * source,
                ))
            })
            .collect();
        let render_poses = bodies
            .iter()
            .map(|(&id, &handle)| {
                let pose = crate::vehicle::from_rapier(physics.rapier().bodies[handle].position());
                (id, pose, pose)
            })
            .collect();
        let item_render_poses = items
            .iter()
            .map(|item| (item.id, item.pose, item.pose))
            .collect();
        let portable_mass = reference
            .bodies
            .iter()
            .filter_map(|body| {
                if !body.dynamic {
                    return None;
                }
                let tanks: Vec<_> = reference
                    .tanks
                    .iter()
                    .filter(|tank| {
                        tank.role == crate::reference::TankRole::Portable
                            && reference
                                .items
                                .iter()
                                .any(|item| item.id == tank.item && item.body == body.id)
                    })
                    .map(|tank| tank.item)
                    .collect();
                if tanks.is_empty() {
                    return None;
                }
                let volume = tank_volume(&items, &tanks);
                Some((bodies[&body.id], body.mass_kg - volume, tanks, body.mass_kg))
            })
            .collect();
        Ok(Self {
            inventory: crate::inventory::InventoryState::new(&reference.inventory),
            inventory_offsets: [None; 4],
            inventory_clutch_conflict: crate::reference::PlayerSettings::defaults(reference)
                .inventory_gear_keys,
            pose: crate::player::pose::PlayerPoseState::new(reference)?,
            body: crate::player::body::BodyState::new(reference)?,
            ragdoll: crate::player::ragdoll::Ragdoll::new(reference)?,
            bodies,
            items,
            interaction,
            player,
            origin,
            attachments,
            render_poses,
            item_render_poses,
            spawned: spawned::SpawnedItems::default(),
            foods: Vec::new(),
            gibs: Default::default(),
            cigarettes: cigarettes::CigaretteRuntime::new(reference)?,
            wear: wear::WearRuntime::new(reference)?,
            portable_mass,
        })
    }

    pub fn sync_origin(&mut self, origin: [f64; 3]) {
        let shift = std::array::from_fn(|i| (origin[i] - self.origin[i]) as f32);
        self.player.rebase(shift);
        self.ragdoll.rebase(shift);
        self.interaction.rebase(shift);
        self.items.rebase(shift);
        self.gibs.rebase(shift);
        for (previous, current) in self.render_pose_pairs() {
            for i in 0..3 {
                previous.position[i] -= shift[i];
                current.position[i] -= shift[i];
            }
        }
        self.origin = origin;
    }

    pub fn begin_step(&mut self) {
        for (previous, current) in self.render_pose_pairs() {
            *previous = *current;
        }
    }
    /// The previous and current render poses of every body and item.
    fn render_pose_pairs(&mut self) -> impl Iterator<Item = (&mut VehiclePose, &mut VehiclePose)> {
        let bodies = self.render_poses.iter_mut().map(|(_, p, c)| (p, c));
        let items = self.item_render_poses.iter_mut().map(|(_, p, c)| (p, c));
        bodies.chain(items)
    }
    pub fn sync_portable_mass(&mut self, physics: &mut PhysicsWorld) {
        for (handle, dry, tanks, current) in &mut self.portable_mass {
            if !physics.body_active(*handle) {
                continue;
            }
            let mass = (*dry + tank_volume(&self.items, tanks)).max(0.001);
            if (mass - *current).abs() > 0.0001 {
                physics.set_body_mass(*handle, mass);
                *current = mass;
            }
        }
    }
    pub fn commit_poses(&mut self, physics: &PhysicsWorld, teleport: bool) {
        self.interaction.decals.commit(physics, teleport);
        let stored = self.inventory.slots.map(|id| {
            id.and_then(|id| self.items.get(id))
                .filter(|i| {
                    matches!(
                        i.location,
                        crate::interaction::items::ItemLocation::Stored(_)
                    )
                })
                .and_then(|i| i.body)
        });
        for (id, previous, current) in &mut self.render_poses {
            if !physics.body_active(self.bodies[id]) && !stored.contains(&Some(self.bodies[id])) {
                continue;
            }
            *current =
                crate::vehicle::from_rapier(physics.rapier().bodies[self.bodies[id]].position());
            if teleport {
                *previous = *current;
            }
        }
        for (id, previous, current) in &mut self.item_render_poses {
            if self
                .items
                .get(*id)
                .is_some_and(|item| item.inactive_pose.is_some())
            {
                continue;
            }
            *current = self.items.get(*id).expect("bound visual item").pose;
            if teleport {
                *previous = *current;
            }
        }
    }

    /// Attached bodies without mechanisms follow the authoritative vehicle pose.
    pub fn sync_attachments(&self, physics: &mut PhysicsWorld, next: bool) {
        for &(handle, parent, local) in &self.attachments {
            if !physics.body_active(handle) || !physics.body_active(parent) {
                continue;
            }
            let parent_pose = if next {
                *physics.rapier().bodies[parent].next_position()
            } else {
                *physics.rapier().bodies[parent].position()
            };
            physics.edit_body(handle, |body| {
                let pose = parent_pose * local;
                if !next {
                    body.set_position(pose, false);
                }
                body.set_next_kinematic_position(pose);
            });
        }
    }
}

/// Total fluid of portable tanks `tanks`.
fn tank_volume(items: &ItemStore, tanks: &[ItemId]) -> f32 {
    tanks
        .iter()
        .map(|id| {
            items
                .get(*id)
                .expect("portable tank")
                .fluid
                .as_ref()
                .expect("portable fluid")
                .finite_total()
        })
        .sum()
}

pub fn collider_item(reference: &ReferenceManifest, mut node: NodeId) -> Option<ItemId> {
    loop {
        if let Some(item) = reference.items.iter().find(|item| item.node == node) {
            return Some(item.id);
        }
        node = reference
            .nodes
            .iter()
            .find(|record| record.id == node)?
            .parent?;
    }
}

/// Body sounds that wait for the next net frame.
const SHARED_SOUNDS: usize = 4;
/// Usual count of live EMPs: sky UFOs and ground UFOs.
const EMP_CAPACITY: usize = 8;

mod crash;
mod inventory;
pub use inventory::InventoryFrame;
mod save;

pub struct GarageRuntime {
    /// The camera and day cycle of the start menu, before the game starts.
    menu_view: menu_view::MenuView,
    pub driving_assists: crate::reference::DrivingAssists,
    pub options: crate::world::GameOptions,
    pub survival: crate::survival::SurvivalState,
    pub hearing: crate::hearing::HearingState,
    pub sleep: crate::sleep::SleepState,
    ambient: crate::audio::AmbientState,
    /// The player `electronicsscript` that ambient EMPs read.
    player_electronics: crate::electronics::Electronics,
    horror: crate::horror::HorrorSound,
    hum: crate::hum::Hum,
    pub food_particles: crate::food_particles::FoodParticles,
    pub gib_particles: crate::damage::gib_particles::GibParticles,
    pub weather: crate::weather::WeatherState,
    weather_bindings: weather::WeatherBindings,
    sky: crate::sky_traffic::SkyTraffic,
    ground_ufos: crate::ground_ufo::GroundUfos,
    /// `PlayerInput` values of this step for a seated UFO.
    ufo_pilot: crate::ground_ufo::UfoPilot,
    pub flyers: crate::flyers::Flyers,
    pub creatures: crate::creatures::Creatures,
    /// Live `empscript` sources in physics space. `sync_emps` adds, moves and removes them.
    pub emps: Vec<crate::electronics::Emp>,
    /// Plane id of each `emps` entry.
    emp_planes: Vec<u32>,
    pub urine: crate::interaction::fluid_contact::FluidContact,
    urine_reference: crate::reference::FluidParticleReference,
    urine_emission: [f32; 2],
    /// Body sounds of the local player for multiplayer peers (source `SSound`). Net drains it each frame.
    pub shared_sounds: Vec<crate::net::message::SoundMessage>,
    /// Items that broke on this guest, with the breakable record. Net sends them to the host.
    pub shared_breaks: Vec<(ItemId, u32)>,
    /// Multiplayer guest. The host alone spawns rabbits and tumbleweeds and breaks items (source
    /// `isServerOrNoMulti`). Net sets it.
    pub net_guest: bool,
    /// Radios whose controls changed, for multiplayer peers (source `SRadio`). Net drains it each frame.
    pub shared_radios: Vec<ItemId>,
    heat_haze: crate::heat_haze::OldHeatHaze,
    /// Source `BOldHeatHaze`. The driver sets it.
    pub old_heat_haze: bool,
    sleep_camera: Option<crate::sleep::SleepCamera>,
    /// Source `otherView()`. A seated outside view, drop cam or map view shows. The driver sets it.
    pub other_view: bool,
    pub audio_motion_revision: u32,
    pub simulation_seconds: f64,
    survival_draw: u64,
    previous_speed: Option<f32>,
    death_protected: bool,
    death_announced: bool,
    pub audio_gameplay: crate::reference::AudioGameplayReference,

    pub audio: Option<crate::audio::AudioState>,
    pub persistent_world: crate::save::PersistentWorld,
    pending_restore: Option<save::PendingRestore>,
    pub lighting: crate::lighting::LightingState,
    /// The main depth of field focus.
    pub focus: crate::depth_of_field::Focus,
    /// The runtime resolves them once from the requested draw distance.
    pub distances: crate::world::DrawDistances,
    pub physics: PhysicsWorld,
    pub area: StartingArea,
    pub stream: WorldStream,
    /// Car copies by vehicle serial. Ordered iteration keeps steps deterministic.
    pub vehicles: BTreeMap<ObjectKey, VehicleRuntime>,
    /// Engine and radiator parts that cars read when mounted.
    pub powertrain: PowertrainParts,
    pub recovering: bool,
    pub startup_aligned: bool,
    recovery_held_bodies: Vec<RigidBodyHandle>,
    /// The vehicle the player last sat in. The HUD and recovery follow it.
    pub vehicle: Option<ObjectKey>,
    pub paused: bool,
    pub started: bool,
    pub attempts: u64,
    /// Kilometers that the player drove as driver.
    pub driven_km: f64,
    pub rebases: u32,
    pub notice: &'static str,
    pub aim: ResolvedAim,
    pub wheels: crate::interaction::wheels::WheelAssemblies,
    crash: crate::crash::CrashState,
    pub driving_input: crate::simulation::input::DrivingInput,
    /// The browser settings. A candidate runtime takes them on commit.
    pub player_settings: crate::reference::PlayerSettings,
    recovery_bodies: Vec<RigidBodyHandle>,
    /// Simulation time of the next under-map check.
    under_map_check: f64,
    frame_poses: crate::boundary::FramePoses,
    /// The first frame carries every condition. Later frames carry changes.
    conditions_sent: bool,
    /// World session whose home cable is done.
    home_cable_session: Option<u64>,
    /// The computers whose screens the renderer has. None before the first frame.
    screens_sent: Option<Vec<ItemId>>,
    /// The computers of this frame, reused each frame.
    screens_scratch: Vec<ItemId>,
    /// World cars whose plate texts the renderer has.
    plates_sent: Vec<crate::world::GeneratedId>,
    pub(crate) structure_views: structure_frame::StructureViews,
    /// The feedback the renderer last received.
    last_feedback: Option<hud::FeedbackStamp>,
    /// The last rendered frame input.
    render_input: crate::boundary::RenderInput,
}

impl GarageRuntime {
    pub fn set_player_settings(&mut self, settings: crate::reference::PlayerSettings) {
        self.player_settings = settings;
        self.area.player.set_settings(settings);
        self.area.interaction.set_settings(settings);
        self.driving_input.set_steering_rate(settings.key_steer);
        self.area.inventory_clutch_conflict = settings.inventory_gear_keys;
    }
    pub fn set_driving_assists(
        &mut self,
        mut requested: crate::reference::DrivingAssists,
    ) -> Result<(), ReferenceError> {
        if self.started && !self.paused {
            return Err(ReferenceError("Driving assists require pause".into()));
        }
        requested.automatic_gears &= requested.automatic_clutch;
        self.driving_assists = requested;
        self.clear_input();
        Ok(())
    }
    /// Applies the game options at once, as `gamemodesettingsscript.SetInMain`.
    pub fn set_game_options(&mut self, options: crate::world::GameOptions) {
        self.options = options;
        self.creatures.peaceful = options.peaceful;
        self.stream.set_peaceful_enemies(options.peaceful_enemies());
    }
    pub fn new(
        reference: &ReferenceManifest,
        mode: crate::world::GameMode,
        draw_distance: f32,
    ) -> Result<Self, ReferenceError> {
        Self::new_candidate(reference, true, mode.seed, mode, draw_distance)
    }
    /// Test runtime in the default game mode at the export default draw distance.
    #[cfg(test)]
    pub fn new_default(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        Self::new(
            reference,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default,
        )
    }
    fn new_candidate(
        reference: &ReferenceManifest,
        stream_world: bool,
        seed: i32,
        mode: crate::world::GameMode,
        draw_distance: f32,
    ) -> Result<Self, ReferenceError> {
        let distances =
            crate::world::DrawDistances::resolve(&reference.world.draw_distance, draw_distance);
        let mut physics = PhysicsWorld::new(reference);

        let mut area = StartingArea::new(reference, &mut physics, [0.0; 3])?;
        let mut stream = WorldStream::new(reference, seed, mode, distances)?;
        if stream_world {
            let spawn = reference.area.player_spawn.translation.map(f64::from);
            stream.request_at(spawn, spawn);
        }
        let mut wheels = crate::interaction::wheels::WheelAssemblies::new(reference)?;
        wheels.refresh_conditions(&area.interaction.conditions);
        wheels.sync(&mut area.items, &mut physics)?;
        area.interaction
            .cleaning
            .sync_wash_colliders(&area.interaction.conditions, &mut physics);
        physics.refresh_queries();
        area.items.sync_poses(&physics);
        area.commit_poses(&physics, true);
        stream.initialize_imported_changes(reference, &area.items, &area.interaction, &physics);
        let lighting = crate::lighting::LightingState::new(&reference.lighting, &distances);
        let audio = reference.audio.playback().ok().map(|r| {
            let mut state = crate::audio::AudioState::new(r.clone(), 1);
            state.bind_game(reference, &area, &physics);
            state.sync_lamps(&reference.lighting, &lighting);
            state
        });
        let weather_bindings = weather::WeatherBindings::new(
            &physics,
            area.player.body_handle(),
            &audio.as_ref().map_or(Vec::new(), |audio| {
                audio
                    .reference
                    .interiors
                    .iter()
                    .map(|i| i.collider)
                    .collect()
            }),
        );
        let urine_visual = reference
            .particles
            .iter()
            .find(|p| p.role == "urine")
            .unwrap();
        let urine_splash = reference
            .particles
            .iter()
            .find(|p| p.role == "urine_splash")
            .unwrap();
        let urine_reference = crate::particles::fluid_reference(urine_visual);
        let urine = crate::interaction::fluid_contact::FluidContact::new(
            &urine_reference,
            urine_visual,
            urine_splash,
            0x70697373,
        );
        let audio_gameplay = reference.audio.gameplay.clone();
        let mut run = Self {
            driving_assists: reference.driving.default_assists,
            options: Default::default(),
            urine,
            urine_reference,
            urine_emission: [0.; 2],
            shared_sounds: Vec::with_capacity(SHARED_SOUNDS),
            shared_breaks: Vec::with_capacity(SHARED_SOUNDS),
            net_guest: false,
            shared_radios: Vec::with_capacity(SHARED_SOUNDS),
            heat_haze: crate::heat_haze::OldHeatHaze::new(&reference.small_items.heat_haze),
            old_heat_haze: false,
            ambient: crate::audio::AmbientState::new(&reference.ambient),
            player_electronics: {
                let r = &reference.ambient.player_electronics;
                crate::electronics::Electronics {
                    override_max_jam: r.override_max_jam,
                    ..crate::electronics::Electronics::new(r.max_output)
                }
            },
            horror: crate::horror::HorrorSound::new(seed as u64),
            hum: crate::hum::Hum::new(&reference.survival.hum, &audio_gameplay),
            food_particles: crate::food_particles::FoodParticles::new(
                &reference.survival.food_particles,
                0x666f_6f64,
            ),
            gib_particles: crate::damage::gib_particles::GibParticles::new(
                &reference.item_prototypes,
                0x6769_6273,
            ),
            weather: crate::weather::WeatherState::new(&reference.weather, seed as u64),
            weather_bindings,
            sky: crate::sky_traffic::SkyTraffic::new(&reference.sky, seed as u64),
            ground_ufos: crate::ground_ufo::GroundUfos::new(
                &reference.sky.ground_ufo,
                &reference.creatures,
                seed as u64,
            ),
            ufo_pilot: crate::ground_ufo::UfoPilot::default(),
            flyers: crate::flyers::Flyers::new(seed as u64),
            creatures: crate::creatures::Creatures::new(reference, seed as u64, 0.)?,
            emps: Vec::with_capacity(EMP_CAPACITY),
            emp_planes: Vec::with_capacity(EMP_CAPACITY),
            survival: crate::survival::SurvivalState::new(&reference.survival),
            hearing: crate::hearing::HearingState::new(reference)?,
            sleep: crate::sleep::SleepState::new(&reference.sleep),
            sleep_camera: None,
            other_view: false,
            audio_motion_revision: 0,
            menu_view: menu_view::MenuView::new(reference, &stream),
            focus: crate::depth_of_field::Focus::new(
                &reference.render_lighting.postprocessing.depth_of_field,
            ),
            simulation_seconds: 0.,
            survival_draw: 0,
            previous_speed: None,
            death_protected: true,
            death_announced: false,
            audio_gameplay,
            audio,
            persistent_world: crate::save::PersistentWorld::new(reference),
            pending_restore: None,
            physics,
            area,
            stream,
            recovering: false,
            startup_aligned: false,
            lighting,
            distances,
            vehicles: BTreeMap::new(),
            powertrain: PowertrainParts::default(),
            vehicle: None,
            paused: false,
            started: false,
            attempts: 0,
            driven_km: 0.,
            rebases: 0,
            notice: "",
            aim: ResolvedAim::default(),
            wheels,
            crash: Default::default(),
            driving_input: crate::simulation::input::DrivingInput::new(
                reference.settings.sliders.key_steer.value(),
                reference.driving.pedal_rate,
                crate::simulation::input::DrivingSettings::defaults(&reference.driving),
            ),
            player_settings: crate::reference::PlayerSettings::defaults(reference),
            recovery_bodies: Vec::with_capacity(reference.items.len()),
            recovery_held_bodies: Vec::with_capacity(reference.items.len()),
            under_map_check: 0.,
            frame_poses: Default::default(),
            conditions_sent: false,
            home_cable_session: None,
            screens_sent: None,
            screens_scratch: Vec::new(),
            plates_sent: Vec::new(),
            structure_views: Default::default(),
            last_feedback: None,
            render_input: Default::default(),
        };
        run.refresh_powertrains();
        Ok(run)
    }

    fn use_light_control(
        &mut self,
        reference: &ReferenceManifest,
        control: crate::reference::LightControlId,
    ) {
        match self
            .stream
            .use_generated_light(control, &mut self.area.items)
        {
            Ok(false) => self.lighting.use_control(&reference.lighting, control),
            Ok(true) => {}
            Err(error) => self.stream.fail(error),
        }
        self.area
            .interaction
            .completed_action(InteractionAction::LightControl(control));
        self.notice = "";
    }

    fn sync_tool_audio(&mut self, reference: &ReferenceManifest) {
        if let Some(audio) = &mut self.audio {
            let cleaning = &self.area.interaction.cleaning;
            let c = &reference.conditions;
            let brushes = c.brushes.iter().map(|b| (b.item, b.tool.voice));
            for (item, voice) in brushes.chain(c.sprays.iter().map(|b| (b.item, b.tool.voice))) {
                if let Some(voice) = voice
                    && let Some(s) = cleaning.signal(item)
                {
                    audio.voice(voice, s.playing, s.voice_gain, 1.);
                }
            }
            if let Some(voice) = c.player_brush.tool.voice {
                let s = cleaning.player_signal();
                audio.voice(voice, s.playing, s.voice_gain, 1.);
            }
            audio.tool_loops.clear();
            for (item, index, _) in cleaning.prototype_tools() {
                let sound = match &c.prototype_tools[index].kind {
                    PrototypeTool::Brush { sound, .. } => sound.as_ref(),
                    PrototypeTool::Spray { sound, .. } => Some(sound),
                };
                let (Some(sound), Some(s), Some(placed)) =
                    (sound, cleaning.signal(item), self.area.items.get(item))
                else {
                    continue;
                };
                if s.playing {
                    let p =
                        to_rapier(placed.pose) * rapier3d::math::Vector::from_array(sound.position);
                    let p = self.stream.origin.absolute(p.to_array());
                    audio.tool_loop(item, sound.voice, s.voice_gain, p);
                }
            }
        }
    }

    pub fn clock_factors(&self, reference: &ReferenceManifest) -> crate::sleep::ClockFactors {
        self.sleep.clock_factors(&reference.sleep)
    }

    fn advance_ambient(&mut self, reference: &ReferenceManifest, dt: f32) {
        let position = self
            .stream
            .origin
            .absolute(self.area.player.body_pose(&self.physics).position);
        // In source order, the zones duck the ambient before napszak reads it.
        if let Some(audio) = &mut self.audio {
            audio.sync_zones(
                self.stream.structure_revision(),
                self.stream.frame().structures,
            );
            self.ambient.duck(audio.zone_step(position));
        }
        // napszakvaltakozas.FixedUpdate reads inSandStorm before the storms update.
        let storm = self.weather.influence().strength.clamp(0., 1.);
        let gains = self.ambient.gains(
            &reference.ambient,
            self.lighting.cycle.output(),
            reference.weather.storm.ambient_decrease * storm,
            dt,
        );
        if let Some(audio) = &mut self.audio {
            use crate::audio::AudioKind;
            audio.source_loop(AudioKind::AmbientDay, 0, gains[0], position);
            audio.source_loop(AudioKind::AmbientNight, 0, gains[1], position);
            audio.source_loop(
                AudioKind::StormInside,
                0,
                reference.weather.storm.volume * storm,
                position,
            );
            audio.sync_rocks(
                self.stream.structure_revision(),
                self.stream.frame().structures,
            );
            audio.rock_step(self.lighting.cycle.output(), position);
        }
        let night = self.lighting.cycle.output().phase == crate::lighting::CyclePhase::Night;
        let horror = self.horror.step(night, self.simulation_seconds);
        if let Some(audio) = &mut self.audio {
            audio.horror(horror, position);
        }
    }

    fn advance_light_fades(&mut self, reference: &ReferenceManifest, dt: f32) {
        self.stream.advance_generated_lamps(dt);
        self.stream.advance_bus_doors(dt, &mut self.physics);
        let cycle = &reference.lighting.cycle;
        self.stream.advance_dials(
            dt,
            (self.lighting.cycle.output().cycle_seconds
                / f64::from(cycle.day_seconds + cycle.night_seconds)) as f32,
            &self.area.items,
        );
        self.stream.advance_levels(
            &reference.small_items.spirit_level,
            dt,
            &self.area.items,
            &self.physics,
        );
        let (items, origin, audio) = (&self.area.items, self.stream.origin, &mut self.audio);
        self.stream
            .advance_music_boxes(dt, |item, pitch, detune, chance| {
                if let (Some(audio), Some(item)) = (audio.as_mut(), items.get(item)) {
                    audio.music_note(origin.absolute(item.pose.position), pitch, detune, chance);
                }
            });
        // metalDetectorScript.Turn stops the beep of a detector that is off.
        if let Some(audio) = audio.as_mut() {
            audio.stop_survival_loops(crate::audio::AudioKind::MetalDetector);
        }
        self.stream
            .advance_detectors(dt, items, &self.physics, |item, position, pitch, volume| {
                if let Some(audio) = audio.as_mut() {
                    audio.pitched_loop(
                        crate::audio::AudioKind::MetalDetector,
                        item.0,
                        volume,
                        Some(pitch),
                        origin.absolute(position),
                    );
                }
            });
        self.lighting.advance_fades(&reference.lighting, dt);
    }

    fn player_step_context(
        &self,
        input: FrameInput,
        availability: crate::interaction::feedback::ActionAvailability,
        reference: &ReferenceManifest,
    ) -> PlayerStepContext {
        use crate::interaction::feedback::ActionMask;
        let picked_up = self.area.interaction.held().is_some();
        let tune = availability.actions.0 & ActionMask::TUNE != 0;
        let mouse_consumed = (input.held & (keys::USE | keys::PRIMARY) != 0
            && self.area.interaction.dragging_mechanism().is_some())
            || (input.held & keys::USE != 0 && tune)
            || ((input.held & keys::USE != 0
                || (input.held & keys::PRIMARY != 0 && availability.lmb_use))
                && availability.actions.0 & ActionMask::CRANK != 0)
            || (input.held & (keys::ROTATE | keys::TRANSLATE) != 0 && picked_up)
            || (input.held & keys::USE != 0
                && availability
                    .item
                    .is_some_and(|id| self.area.interaction.mechanisms.contains(id)));
        let movement_multiplier = self.area.interaction.carry.body().map_or(1.0, |body| {
            let mass = self.physics.authored_body_mass(body);
            ((mass - reference.player.carry_max_weight)
                / (reference.player.carry_min_weight - reference.player.carry_max_weight))
                .clamp(0.0, 1.0)
        });
        let slideable = self
            .area
            .interaction
            .dragging_mechanism()
            .is_some_and(|id| self.area.interaction.mechanisms.slideable(id));
        let binoculars = self.area.interaction.binoculars.active().is_some();
        // Source `CanDuck` is false while the player hums.
        let duck_allowed = !self.hum.humming()
            && PlayerStepContext::can_duck(input, tune || binoculars, picked_up);
        // fpscontroller `scrubbing`: `drotkefe.Use` in the hand, or pointing.
        let scrubbing = self.area.interaction.scrubbing()
            || input.player_held & crate::simulation::player_keys::POINT != 0;
        PlayerStepContext {
            scrubbing,
            duck_allowed,
            wheel_posture_allowed: duck_allowed && !slideable,
            mouse_consumed,
            picked_up,
            recenter_consumed: picked_up && (input.held | input.pressed) & keys::RECENTER != 0,
            movement_multiplier,
            binoculars,
        }
    }

    pub fn step(&mut self, reference: &ReferenceManifest, mut input: FrameInput) {
        self.wheels
            .refresh_conditions(&self.area.interaction.conditions);
        if let Err(error) = self.sync_world_activity(reference) {
            self.stream.fail(ReferenceError(error.to_string()));
        }
        // The check reads the stream request, so the request is the current position.
        let held = !self.started || self.paused || self.recovering || {
            self.request_focus();
            !self.world_supported()
        };
        if held {
            // A held step draws the last pose. Interpolation toward it would shake the car.
            self.area.begin_step();
            self.clear_input();
            #[cfg(target_arch = "wasm32")]
            if crate::cpu_profile::enabled() {
                crate::host::game_cpu_clear_input();
            }
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if crate::cpu_profile::enabled() {
            crate::host::game_cpu_input_consumed();
        }
        let factors = self.clock_factors(reference);
        let sleep = self.sleep.output();
        self.lighting.cycle.advance(
            &reference.lighting.cycle,
            crate::lighting::CycleAdvance {
                simulation_delta: FIXED_DT,
                cycle_extra: FIXED_DT * factors.cycle_extra_per_sim_second,
                cloud_scale: factors.cloud_scale,
                sleep_progress: (sleep.phase == crate::sleep::SleepPhase::Sleeping)
                    .then_some(sleep.progress),
            },
        );
        self.urine_emission = [0.; 2];
        let sleep_held = input.held & keys::SLEEP != 0;
        if self.sleeping() {
            input = FrameInput::default();
        } else {
            self.process_toilet(reference, input, FIXED_DT, factors.fluid_scale);
        }
        let needs = self.needs_output();
        if needs.dead {
            input = FrameInput {
                mouse: input.mouse.map(|v| v * needs.look_scale),
                ..FrameInput::default()
            };
            self.clear_input();
        }
        self.step_typing(&mut input);
        self.area
            .interaction
            .finish_cancelled_input(&mut self.area.player, &mut self.physics);
        self.step_wear(&mut input);
        let smoking = self.step_cigarettes(reference, &mut input);
        self.step_hum(reference, &mut input, smoking);
        for (key, vehicle) in &mut self.vehicles {
            vehicle.previous = self.persistent_world.absolute_vehicle_position(
                &self.physics,
                &self.stream.origin,
                key,
            );
            // Spawn, load and restore set velocity before this point, so a new car does not crash.
            vehicle.last_speed = crash::forward_kmh(&self.physics, key);
        }

        self.area.begin_step();
        let preliminary_aim = if self.sleeping() {
            ResolvedAim::default()
        } else {
            self.area.interaction.resolve(
                &self.physics,
                self.area.player.view(&self.physics).camera_pose,
                &self.area.items,
            )
        };
        let preliminary_availability = self.area.interaction.available_actions(
            &preliminary_aim,
            &self.area.player,
            &self.area.items,
            &self.physics,
            self.survival.output(),
        );
        let view_context = self.player_step_context(input, preliminary_availability, reference);
        self.area.player.step_view(input, view_context, FIXED_DT);
        let view = self.area.player.view(&self.physics);
        let aim = if self.sleeping() || view.ray_actions_blocked {
            ResolvedAim::default()
        } else {
            self.area
                .interaction
                .resolve(&self.physics, view.camera_pose, &self.area.items)
        };
        if !self.aim.same_actions(&aim) || input.pressed & (keys::GRAB | keys::USE) != 0 {
            self.notice = "";
        }
        self.aim = aim;
        let focus = &reference.render_lighting.postprocessing.depth_of_field;
        let hit = crate::depth_of_field::ray(&self.physics, view.camera_pose, focus);
        self.focus
            .step(focus, hit, self.lighting.cycle.output(), FIXED_DT);
        let previous_seat = self.area.player.seat_body();
        let seated = self.area.player.seated();
        let outcome = self.area.interaction.step(
            input,
            aim,
            &mut self.area.player,
            &mut self.area.body,
            &mut self.area.items,
            &mut self.physics,
            FIXED_DT,
            self.survival.output(),
            factors.fluid_scale,
        );
        if self.area.interaction.lmb_owner().is_some() {
            // fpscontroller noLMBUse. LMB on the held tool does not use the aimed item.
            input.drop_keys(keys::PRIMARY);
        }
        let action = outcome.action;
        if action.is_none()
            && (input.pressed | input.released) & keys::GRAB != 0
            && let Some(block) = aim.installation_block.or(aim.grab_block)
        {
            self.notice = block.notice();
        }
        match action {
            Some(InteractionAction::Consume(item)) => {
                if self.consume(reference, item).is_err() {
                    self.notice = "Cannot eat this item.";
                }
            }
            Some(InteractionAction::Drink {
                source,
                requested_liters,
            }) => {
                if self.drink(reference, source, requested_liters).is_err() {
                    self.notice = "Cannot drink from this item.";
                }
            }
            Some(InteractionAction::DrinkContact {
                source,
                poured_liters,
                fractions,
            }) => {
                if self
                    .drink_contact(reference, source, poured_liters, fractions)
                    .is_err()
                {
                    self.notice = "Cannot drink this fluid.";
                }
            }
            Some(InteractionAction::LightControl(control)) => {
                self.use_light_control(reference, control)
            }
            Some(InteractionAction::DialTune(knob, direction)) => {
                let (audio, physics) = (&mut self.audio, &self.physics);
                let bus_door = self.stream.use_bus_door_button(knob, |key, shot| {
                    if let Some(audio) = audio.as_mut()
                        && physics.vehicle_keys().any(|k| k == key)
                    {
                        let pose = *physics.rapier().bodies[physics.chassis_handle(key)].position();
                        audio.car_shot(key, shot, pose);
                    }
                });
                if !bus_door
                    && !self
                        .stream
                        .switch_display(knob, reference, &self.area.items)
                    && !self.stream.tune_dial(knob, direction, &mut self.area.items)
                    && !self.stream.switch_detector(knob, &mut self.area.items)
                    && let Some((radio, click)) =
                        self.stream
                            .radio_control(knob, direction, &mut self.area.items)
                {
                    if !self.shared_radios.contains(&radio)
                        && self.shared_radios.len() < self.shared_radios.capacity()
                    {
                        self.shared_radios.push(radio);
                    }
                    if click
                        && let (Some(audio), Some(item)) =
                            (&mut self.audio, self.area.items.get(radio))
                    {
                        let origin = self.area.origin;
                        audio.radio_click_at(std::array::from_fn(|i| {
                            origin[i] + f64::from(item.pose.position[i])
                        }));
                    }
                }
            }
            Some(InteractionAction::Crank(knob, look)) => {
                self.stream.crank_music_box(knob, look);
            }
            Some(InteractionAction::Ufo(target, value)) => {
                self.ground_ufos.use_control(target, value);
            }
            Some(InteractionAction::Wheel(action)) => {
                if let Some(notice) = self.area.interaction.complete_wheel_action(
                    action,
                    aim,
                    &mut self.wheels,
                    &self.area.player,
                    &mut self.area.items,
                    &mut self.physics,
                ) {
                    self.notice = notice;
                }
            }
            _ => {}
        }
        self.computer_actions(input);
        self.play_computer_sounds();
        self.play_computer_notes(&reference.computer.computer);
        while let Some((source, poured_liters, fractions)) =
            self.area.interaction.take_drink_contact()
        {
            if self
                .drink_contact(reference, source, poured_liters, fractions)
                .is_err()
            {
                self.notice = "Cannot drink this fluid.";
            }
        }
        if let Some(action) = self.area.interaction.mechanisms.take_control_action() {
            match action {
                crate::interaction::mechanisms::ControlAction::ToggleHandbrake(switch) => {
                    if let Some(vehicle) = self
                        .vehicles
                        .values_mut()
                        .find(|v| v.handbrake_switch == Some(switch))
                    {
                        vehicle.handbrake = !vehicle.handbrake;
                        if let Some(key) = self.area.player.seat_vehicle().cloned()
                            && self.area.player.driving()
                            && self.vehicles[&key].handbrake_switch == Some(switch)
                        {
                            self.reach_seated_car(
                                reference,
                                &key,
                                crate::simulation::reach::handbrake,
                            );
                        }
                    }
                }
                crate::interaction::mechanisms::ControlAction::CycleHeadlights(switch) => {
                    if let Some((key, record)) = use_light_switch(&mut self.vehicles, switch) {
                        self.car_switch_sound(reference, &key, record);
                        self.reach_seated_car(reference, &key, crate::simulation::reach::lights);
                    }
                }
                _ => {}
            }
        }
        self.area.sync_portable_mass(&mut self.physics);
        if seated
            && input.pressed & keys::USE != 0
            && action.is_none()
            && self.area.interaction.dragging_mechanism().is_none()
            && self.area.interaction.tuning_opening().is_none()
        {
            self.notice = if self.area.player.seated() {
                "Look toward a clear exit."
            } else {
                ""
            };
        } else if input.mouse != [0.0; 2] {
            self.notice = "";
        }
        let mut walking_input = input;
        walking_input.mouse = outcome.look_mouse;
        let final_availability = self.area.interaction.available_actions(
            &aim,
            &self.area.player,
            &self.area.items,
            &self.physics,
            self.survival.output(),
        );
        let context = self.player_step_context(walking_input, final_availability, reference);
        if needs.dead {
            self.area.player.clear_triggers();
        } else {
            self.area
                .player
                .scan_triggers(&self.physics, self.stream.trigger_roles());
        }
        if self.area.ragdoll.active() {
            self.follow_ragdoll();
        } else {
            self.area
                .player
                .step_movement(walking_input, context, &mut self.physics, FIXED_DT);
        }
        self.apply_hazards(reference);
        self.area.sync_attachments(&mut self.physics, true);
        self.area
            .interaction
            .sync_parts(&mut self.area.items, &mut self.physics)
            .expect("bound parts");
        if let Some(vehicle) = self.area.player.seat_vehicle()
            && self.vehicle.as_ref() != Some(vehicle)
        {
            self.vehicle = Some(vehicle.clone());
        }
        let vehicle_input = if previous_seat != self.area.player.seat_body() {
            self.clear_driving_input();
            // Pads stay, so a button held across the seat change gives no edge.
            FrameInput {
                gamepad: input.gamepad,
                ..FrameInput::default()
            }
        } else {
            input
        };
        let driving = self.area.player.driving();
        if input.player_pressed & crate::simulation::player_keys::MOUSE_DRIVE != 0 {
            self.driving_input.toggle_mouse();
        }
        // The car driver seat and item seats take the driving input. Only sleds read it on item seats.
        let car = driving
            .then(|| self.area.player.seat_vehicle())
            .flatten()
            .and_then(|key| {
                let record = self.vehicles.get(key)?.record(reference);
                Some(record.key_steer(self.physics.snapshot(key).speed_mps * 3.6))
            });
        let driving_output = self.driving_input.step(
            if driving {
                self.area.player.seat_body()
            } else {
                self.area.player.item_seat_body()
            },
            &vehicle_input,
            car,
            self.driving_assists.automatic_gears,
        );
        let controls = driving_output.controls;
        self.ufo_pilot = crate::ground_ufo::UfoPilot {
            throttle: controls.throttle,
            brake: controls.brake,
            steer: controls.steer,
            up: vehicle_input.held & keys::JUMP != 0,
            down: vehicle_input.held & keys::CROUCH != 0,
        };
        // Source `carscript` cycles off, low, high.
        if (driving || self.area.player.driving2())
            && input.pressed & keys::LIGHTS != 0
            && let Some(key) = self.vehicle.clone()
            && let Some(vehicle) = self.vehicles.get_mut(&key)
        {
            vehicle.cycle_headlights();
            let prototype = vehicle.prototype;
            self.car_switch_sound(reference, &key, prototype);
            self.reach_seated_car(reference, &key, crate::simulation::reach::lights);
        }
        if (driving || self.area.player.driving2())
            && input.player_pressed & crate::simulation::player_keys::HANDBRAKE != 0
            && let Some(key) = self.vehicle.clone()
            && let Some(vehicle) = self.vehicles.get_mut(&key)
        {
            vehicle.handbrake = !vehicle.handbrake;
            if driving {
                self.reach_seated_car(reference, &key, crate::simulation::reach::handbrake);
            }
        }
        self.advance_electronics(reference, FIXED_DT);
        self.refresh_powertrains();
        // Taking the map lets each car step borrow the runtime without allocation.
        let mut vehicles = std::mem::take(&mut self.vehicles);
        let mut world = CarWorld {
            climate: self.engine_climate(reference),
            physics: &mut self.physics,
            items: &mut self.area.items,
            interaction: &self.area.interaction,
            player: &self.area.player,
            body: &mut self.area.body,
            powertrain: &mut self.powertrain,
            audio: &mut self.audio,
            origin: &self.stream.origin,
            assists: self.driving_assists,
            attempts: &mut self.attempts,
        };
        for (key, vehicle) in &mut vehicles {
            let input = CarInput {
                frame: vehicle_input,
                driven: driving && self.vehicle.as_ref() == Some(key),
                driving: driving_output,
            };
            vehicle.step(reference, key, &mut world, input);
        }
        self.vehicles = vehicles;
        for (key, vehicle) in &self.vehicles {
            if let ObjectKey::Generated(id) = key {
                self.stream.set_car_lights(id, vehicle.lights.lights());
            }
        }
        let (items, vehicles) = (&self.area.items, &self.vehicles);
        self.stream
            .advance_trailer_lights(items, &self.area.interaction, |car| {
                vehicles
                    .get(&items.object_key(car))
                    .map_or_else(Default::default, |vehicle| vehicle.light_input(reference))
            });
        self.step_loose_engines(reference);
        let sled_seat = match (
            self.area.player.seat_owner(),
            self.area.player.item_seat_source(),
        ) {
            (Some(crate::player::SeatOwner::Item(owner)), Some(source)) => Some((owner, source)),
            _ => None,
        };
        self.stream
            .step_sleds(&mut self.physics, sled_seat, driving_output.controls);
        self.stream.step_trailers(&mut self.physics);
        self.stream.step_followers(&self.physics);
        self.advance_urine(reference, FIXED_DT);
        self.advance_ambient(reference, FIXED_DT);
        self.advance_weather(reference, FIXED_DT);
        self.advance_sky(reference, FIXED_DT);
        self.advance_flyers(reference, FIXED_DT);
        self.advance_creatures(reference, FIXED_DT);
        self.area
            .interaction
            .mechanisms
            .apply_reactions(&mut self.physics);
        self.physics.step_physics();
        self.step_crashes(reference);
        self.area
            .player
            .step_head_bob(walking_input, needs.dead, &self.physics, FIXED_DT);
        self.sample_survival_speed(reference);
        self.area.interaction.sync_mechanisms(&mut self.physics);
        self.area.sync_attachments(&mut self.physics, false);
        self.area
            .interaction
            .sync_parts(&mut self.area.items, &mut self.physics)
            .expect("bound parts");
        self.wheels
            .sync(&mut self.area.items, &mut self.physics)
            .expect("validated wheel bindings");
        self.physics.refresh_queries();
        if self
            .area
            .interaction
            .storage
            .advance(
                &mut self.area.items,
                &mut self.physics,
                &self.area.interaction.parts,
                &self.area.interaction.mechanisms,
                self.area.player.item_seat_body(),
                FIXED_DT,
            )
            .expect("bound storage")
        {
            self.wheels
                .sync(&mut self.area.items, &mut self.physics)
                .expect("validated wheel bindings");
        }
        self.area.sync_inventory();
        self.area.sync_equipment(reference, &mut self.physics);
        self.step_body(reference);
        self.area.sync_worn(&mut self.physics);
        self.area.items.sync_poses(&self.physics);
        self.break_targets(reference);
        self.area.commit_poses(&self.physics, false);
        if let Some(audio) = &mut self.audio {
            audio.fixed_step(&self.area, &self.physics, &self.stream, FIXED_DT);
            audio.hits(
                reference,
                &self.area,
                &self.vehicles,
                &self.physics,
                FIXED_DT,
            );
            let stream = &mut self.stream;
            let biome_override = stream.mode().biome_override();
            audio.footsteps(
                &self.area,
                &self.physics,
                walking_input.held & keys::RUN != 0,
                |position| crate::world::biome(stream.terrain_color(position), biome_override),
            );
            while let Some(sound) = self.area.interaction.pop_sound() {
                match sound {
                    crate::interaction::InteractionSound::Action(action) => {
                        audio.action(action, audio.game_seconds)
                    }
                    crate::interaction::InteractionSound::Cap {
                        id,
                        previous,
                        current,
                        tune,
                    } => {
                        if audio.imported_cap(id) {
                            audio.cap(id, previous, current, tune);
                        } else if let Some((clips, cap)) = self.area.interaction.cap_clips(id)
                            && let Some(item) = self.area.items.get(cap)
                        {
                            let position = self.stream.origin.absolute(item.pose.position);
                            audio.generated_cap(clips, previous, current, tune, position);
                        }
                    }
                }
            }
            // szankoScript.Update sets the slide volume and pitch of each sled in use.
            audio.stop_survival_loops(crate::audio::AudioKind::Sled);
            let origin = self.stream.origin;
            self.stream
                .sled_sounds(&self.physics, |item, volume, pitch, position| {
                    audio.pitched_loop(
                        crate::audio::AudioKind::Sled,
                        item.0,
                        volume,
                        pitch,
                        origin.absolute(position),
                    )
                });
        }
        self.step_car_sounds(reference, input);
        let driven = self
            .area
            .player
            .seat_vehicle()
            .filter(|_| self.area.player.driving())
            .cloned();
        for (key, vehicle) in &mut self.vehicles {
            let current = self.persistent_world.absolute_vehicle_position(
                &self.physics,
                &self.stream.origin,
                key,
            );
            let previous = vehicle.previous;
            let distance_m =
                ((current[0] - previous[0]).powi(2) + (current[2] - previous[2]).powi(2)).sqrt();
            vehicle.trip_m += distance_m;
            if driven.as_ref() == Some(key) {
                self.driven_km += distance_m / 1000.;
            }
            if vehicle.kmh_slots.is_empty() {
                continue;
            }
            // Source carscript.Update reads the meter of the part in each kmhmeters slot.
            let record = world_vehicle(reference, vehicle.prototype);
            let sample = instrument_sample(
                &self.area.items,
                &self.physics,
                &self.powertrain,
                key,
                vehicle,
                record,
                vehicle.requested_headlights,
            );
            for &(row, slot) in &vehicle.kmh_slots {
                if let Some(item) = self.area.interaction.parts.slot_item(slot)
                    && let Some(gauge) = self.powertrain.gauge_mut(item)
                {
                    gauge.state.kmh_update(
                        &gauge.record,
                        &record.kmh_meters[row],
                        &record.gauge_limits,
                        &sample,
                        (distance_m / 1000.) as f32,
                        FIXED_DT,
                    );
                }
            }
        }
        for gauge in self.powertrain.gauges_mut() {
            if gauge.state.advance(&gauge.record, FIXED_DT) {
                self.stream
                    .mark_presentation(&gauge.parent, &mut gauge.slot_hint);
            }
        }

        self.advance_under_map();
        let focus = self.request_focus();
        let absolute = self.stream.origin.absolute(focus);
        // napszak refreshes CurrentColor under the player each second.
        if self.lighting.cycle.needs_terrain_color(0.) {
            let color = self.stream.terrain_color(absolute);
            self.lighting
                .cycle
                .set_terrain_color(&reference.lighting.cycle, &color, 0.);
        }
        self.persistent_world
            .sync(
                reference,
                &mut self.area,
                &mut self.wheels,
                &self.stream,
                &mut self.physics,
            )
            .expect("persistent world bindings");
        self.rebase_to(focus);
        self.stream.capture_changes(
            reference,
            &mut self.area.items,
            &self.area.interaction,
            &self.physics,
        );
        self.persistent_world
            .sync(
                reference,
                &mut self.area,
                &mut self.wheels,
                &self.stream,
                &mut self.physics,
            )
            .expect("persistent world bindings");
        self.advance_light_fades(reference, FIXED_DT);
        self.advance_cigarettes();
        self.heat_haze
            .step(self.old_heat_haze, FIXED_DT, &self.physics);
        self.food_particles
            .advance(FIXED_DT, self.stream.origin.offset, &self.physics);
        self.gib_particles
            .advance(FIXED_DT, self.stream.origin.offset, &self.physics);
        self.advance_needs(reference, FIXED_DT, factors.need_scale);
        let was_sleeping = self.sleeping();
        let output = self.sleep.step(
            &reference.sleep,
            crate::sleep::SleepInput {
                held: sleep_held,
                active: !self.paused,
                camera_allowed: !self.other_view || !self.area.player.seated(),
                dead: self.needs_output().dead,
                show_text: self.area.player.show_sleep_text(),
            },
            self.simulation_seconds,
            FIXED_DT,
        );
        self.sync_sleep_transition(reference, was_sleeping);
        self.lighting.cycle.set_sleep_progress(
            &reference.lighting.cycle,
            (output.phase == crate::sleep::SleepPhase::Sleeping).then_some(output.progress),
        );
    }

    /// Planes run in every state, also while the player sits in a car.
    fn advance_sky(&mut self, reference: &ReferenceManifest, dt: f32) {
        self.sky
            .advance(&reference.sky, self.simulation_seconds, dt);
        let anchor = self.sky_anchor(reference);
        let origin = self.stream.origin.offset;
        self.sky.advance_contrails(
            dt,
            std::array::from_fn(|i| origin[i] - anchor[i]),
            &self.physics,
        );
        let player = self
            .stream
            .origin
            .absolute(self.area.player.body_pose(&self.physics).position);
        let seated = match self.area.player.seat_owner() {
            Some(crate::player::SeatOwner::Ufo(id)) => Some((*id, self.ufo_pilot)),
            _ => None,
        };
        let stream = &self.stream;
        let mut world = crate::ground_ufo::UfoWorld {
            physics: &mut self.physics,
            interaction: &mut self.area.interaction,
            items: &mut self.area.items,
            origin: &stream.origin,
        };
        self.ground_ufos
            .advance(
                &reference.sky,
                self.lighting.cycle.output().phase,
                player,
                seated,
                !self.net_guest,
                self.simulation_seconds,
                dt,
                &mut world,
                |position, above| stream.road_point(position, above),
            )
            .expect("validated ground UFO colliders");
        self.ground_ufos
            .place(&reference.sky.ground_ufo, anchor, &world);
        self.sync_emps(reference, anchor);
        if let Some(audio) = &mut self.audio {
            audio.sky(
                self.sky.planes().iter().chain(self.ground_ufos.planes()),
                anchor,
            );
        }
    }

    /// Sky planes, then ground UFOs, relative to the sky anchor.
    pub fn planes(&self) -> impl Iterator<Item = &crate::sky_traffic::Plane> {
        self.sky.planes().iter().chain(self.ground_ufos.planes())
    }

    /// One `emps` entry follows each plane whose prefab carries an empscript.
    fn sync_emps(&mut self, reference: &ReferenceManifest, anchor: [f64; 3]) {
        let mut i = 0;
        while i < self.emp_planes.len() {
            let id = self.emp_planes[i];
            if self.planes().any(|p| p.id == id) {
                i += 1;
            } else {
                self.emp_planes.swap_remove(i);
                self.emps.swap_remove(i);
            }
        }
        let origin = self.stream.origin;
        for plane in self.sky.planes().iter().chain(self.ground_ufos.planes()) {
            let Some(emp) = &reference.sky.prefabs[plane.prefab].emp else {
                continue;
            };
            let position =
                rapier3d::math::Vector::from_array(origin.local(std::array::from_fn(|i| {
                    anchor[i] + f64::from(plane.offset[i])
                })));
            match self.emp_planes.iter().position(|id| *id == plane.id) {
                Some(i) => self.emps[i].position = position,
                None => {
                    self.emp_planes.push(plane.id);
                    self.emps.push(crate::electronics::Emp::new(emp, position));
                }
            }
        }
    }

    fn advance_flyers(&mut self, reference: &ReferenceManifest, dt: f32) {
        let flyer = &reference.sky.flyer;
        self.flyers.sync(
            flyer,
            self.simulation_seconds,
            self.stream.structure_revision(),
            self.stream.frame().structures,
            &self.area.items,
            &self.physics,
        );
        self.flyers.step(
            flyer,
            self.simulation_seconds,
            dt,
            &self.area.items,
            &mut self.physics,
        );
        if let Some(audio) = &mut self.audio {
            let (items, physics, origin) = (&self.area.items, &self.physics, &self.stream.origin);
            audio.flyers(
                flyer.voice,
                self.flyers.squeaks(),
                self.simulation_seconds,
                |item| {
                    let body = physics.rapier().bodies.get(items.get(item)?.body?)?;
                    Some(origin.absolute(body.translation().to_array()))
                },
            );
        }
    }

    /// Rabbits run in every state. Source `aispawn` sits on the Player.
    fn advance_creatures(&mut self, reference: &ReferenceManifest, dt: f32) {
        self.bind_rabbit_sounds();
        let player = &self.area.player;
        let body = crate::vehicle::to_rapier(player.body_pose(&self.physics));
        let head = rapier3d::math::Vector::from_array(player.focus_position(&self.physics));
        let (stream, origin) = (&self.stream, self.stream.origin);
        let supports = |p: rapier3d::math::Vector| {
            let p = origin.absolute(p.to_array());
            stream.supports(crate::world::WorldBounds {
                min: p.map(|v| v - 2.),
                max: p.map(|v| v + 2.),
            })
        };
        let important = |p: rapier3d::math::Vector| {
            stream.important_distance(origin.absolute(p.to_array()), None)
        };
        let d = &stream.distances;
        let distances = crate::creatures::PlaceDistances {
            remove: d.item_remove_dist.powi(2),
            spawn: d.item_spawn_dist.powi(2),
            freeze: d.item_freeze_dist.powi(2),
            unfreeze: d.item_unfreeze_dist.powi(2),
        };
        self.creatures.step(
            &reference.creatures.rabbit,
            &mut self.physics,
            &crate::creatures::StepInput {
                now: self.simulation_seconds,
                dt,
                player: crate::creatures::spawner::Player {
                    position: body.translation,
                    rotation: body.rotation,
                },
                head: (head, player.body_handle()),
                hum: self.hum.stimulus(&reference.survival.hum),
                origin: &origin,
                supports: &supports,
                important: &important,
                distances,
                spawns: !self.net_guest,
                emps: &self.emps,
                bodies: self.ground_ufos.stimuli(),
            },
        );
        self.creatures
            .humans
            .collide(&reference.creatures.human, &self.physics);
        self.settle_humans(reference);
        self.place_humans(reference);
        self.step_humans(reference, dt);
        if self.creatures.place_remove_due(self.simulation_seconds) {
            self.place_remove_creatures(reference, distances);
        }
        self.settle_rabbits(reference);
        let r = &reference.creatures.human;
        if let Some(audio) = &mut self.audio {
            let sounds = self
                .creatures
                .humans
                .sounds(r, &self.physics, self.simulation_seconds);
            audio.humans(r.voice.voice, sounds, &self.stream.origin);
        }
    }

    pub fn sky(&self) -> &crate::sky_traffic::SkyTraffic {
        &self.sky
    }

    /// Absolute anchor of the plane offsets: player body + up * height.
    pub fn sky_anchor(&self, reference: &ReferenceManifest) -> [f64; 3] {
        let mut anchor = self
            .stream
            .origin
            .absolute(self.area.player.body_pose(&self.physics).position);
        anchor[1] += reference.sky.height as f64;
        anchor
    }

    /// Gauge inputs of car `key`.
    pub fn instrument_sample(
        &self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
    ) -> crate::simulation::instruments::InstrumentSample {
        let vehicle = &self.vehicles[key];
        instrument_sample(
            &self.area.items,
            &self.physics,
            &self.powertrain,
            key,
            vehicle,
            vehicle.record(reference),
            vehicle.requested_headlights,
        )
    }
    /// Advances the gauges of every car by render time `dt`. `initialize` jumps to the targets.
    fn advance_instruments(&mut self, reference: &ReferenceManifest, dt: f32, initialize: bool) {
        let Self {
            vehicles,
            area,
            physics,
            powertrain,
            distances,
            ..
        } = self;
        // Gauges beyond the draw distance are not presented. They catch up when the car returns.
        let focus = rapier3d::math::Vector::from_array(area.player.focus_position(physics));
        let seated = area.player.seat_vehicle();
        for (key, vehicle) in vehicles.iter_mut() {
            let record = vehicle.record(reference);
            let Some(r) = record.instruments.as_ref() else {
                continue;
            };
            let position = physics.rapier().bodies[physics.chassis_handle(key)].translation();
            if !initialize
                && seated != Some(key)
                && (position - focus).length() > distances.draw_distance
            {
                continue;
            }
            let sample = instrument_sample(
                &area.items,
                physics,
                powertrain,
                key,
                vehicle,
                record,
                vehicle.requested_headlights,
            );
            let Some(state) = vehicle.instruments.as_mut() else {
                continue;
            };
            if initialize {
                state.initialize_from_sample(r, &record.gauge_limits, sample);
            } else {
                state.update(r, &record.gauge_limits, sample, dt);
            }
        }
    }

    /// Work of each rendered frame. Gauges and gear levers move by render time.
    /// `fill_frame` advances the audio with the same input.
    pub fn advance_render_frame(
        &mut self,
        reference: &ReferenceManifest,
        input: crate::boundary::RenderInput,
    ) {
        self.render_input = input;
        if self.started && !self.paused {
            self.advance_instruments(reference, input.dt, false);
            // Source `carscript.Update` eases the lever once per frame by scaled frame time.
            self.advance_levers(reference, input.dt);
        }
    }
    /// Source `carscript.Update`. Eases each car gear lever and control pedal by render time `dt`.
    fn advance_levers(&mut self, reference: &ReferenceManifest, dt: f32) {
        for vehicle in self.vehicles.values_mut() {
            vehicle.advance_controls(reference, dt);
        }
    }

    /// Car and engine part sounds of every copy in range of the player.
    fn step_car_sounds(&mut self, reference: &ReferenceManifest, input: FrameInput) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        let origin = self.stream.origin.offset;
        audio.begin_cars(origin);
        let listener =
            rapier3d::math::Vector::from_array(self.area.player.focus_position(&self.physics));
        let ambient_temperature = self.lighting.cycle.output().ambient_temperature;
        let seated = self.area.player.seat_vehicle();
        let controls = self.area.player.driving() || self.area.player.driving2();
        let (items, physics) = (&self.area.items, &self.physics);
        let engine_input = |part, mixer| engine_sound_input(part, items, physics, mixer);
        for (key, vehicle) in &self.vehicles {
            let chassis = self.physics.chassis_handle(key);
            let pose = *self.physics.rapier().bodies[chassis].position();
            if !audio.car_audible((pose.translation - listener).length()) {
                continue;
            }
            let record = vehicle.record(reference);
            let mixer = match key {
                ObjectKey::Generated(placement) => record
                    .audio
                    .as_ref()
                    .map(|a| audio.world_car_mixer(&a.route, placement)),
                ObjectKey::Imported(_) => None,
            };
            if let Some(car) = record.audio.as_ref() {
                let active = self.physics.body_active(chassis);
                audio.car_sound(
                    key,
                    crate::audio::cars::CarSoundInput {
                        audio: car,
                        pose,
                        active,
                        horn: active
                            && ((controls && seated == Some(key) && input.held & keys::HORN != 0)
                                || vehicle.net.remote.is_some_and(|r| r.horn > 0.)),
                        horn_hit: vehicle.horn_hit.map(|h| h.gain()),
                        ambient_temperature,
                        wheels: &vehicle.wheel_effects,
                        boil: vehicle.coolant_steam.boil(),
                        skid: &reference.skid,
                        mixer,
                    },
                    FIXED_DT,
                );
            }
            if let Some(input) = vehicle
                .engine()
                .and_then(|item| self.powertrain.engine(item))
                .and_then(|part| engine_input(part, mixer))
            {
                audio.engine_sound(input);
            }
        }
        // Loose engine parts keep sounding while they run.
        for part in self.powertrain.engines() {
            if self
                .vehicles
                .values()
                .any(|v| v.engine() == Some(part.item))
            {
                continue;
            }
            if let Some(input) = engine_input(part, None)
                && audio.car_audible((input.pose.translation - listener).length())
            {
                audio.engine_sound(input);
            }
        }
        audio.cars.end();
    }

    /// The `carscript` switch sound of car `key`: ignition and light switch.
    fn car_switch_sound(
        &mut self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
        prototype: usize,
    ) {
        car_sound(
            &mut self.audio,
            &self.physics,
            reference,
            key,
            prototype,
            |a| a.switch,
        );
    }

    /// The engine rpm of a car, 0 without an engine.
    pub fn engine_rpm(&self, key: &ObjectKey) -> f32 {
        self.vehicles
            .get(key)
            .and_then(|vehicle| vehicle.engine())
            .and_then(|item| self.powertrain.engine(item))
            .map_or(0., |engine| engine.state.rpm)
    }

    /// Source breakablescript: contacts and hazard triggers of this step, then Break in break order.
    fn break_targets(&mut self, reference: &ReferenceManifest) {
        self.stream.step_breakables(
            reference,
            &self.area.items,
            &self.area.spawned,
            &mut self.physics,
            FIXED_DT,
        );
        while let Some((kind, record)) = self.stream.take_break() {
            let rules = &reference.breakables[record];
            let item = match kind {
                crate::damage::BreakKind::Item(item) => item,
                // Cacti have no tosaveitemscript. Source Break throws at tosave.PreDestroy, after
                // the gib spawns. The prop stays and does not break again.
                crate::damage::BreakKind::Prop { parent } => {
                    let Some(pose) = self.stream.prop_pose(&parent) else {
                        continue;
                    };
                    let pose = crate::vehicle::VehiclePose {
                        position: self.stream.origin.local(pose.position),
                        rotation: pose.rotation,
                    };
                    self.spawn_gib(reference, rules, pose, false);
                    continue;
                }
                // Lamp triggers have no gib.
                crate::damage::BreakKind::Lamp { parent, control } => {
                    let position = self
                        .stream
                        .break_lamp(reference, &mut self.physics, &parent, control, record)
                        .expect("bound lamp");
                    if let Some(clip) = self.stream.pick_break_clip(rules)
                        && let Some(audio) = &mut self.audio
                        && let Some(position) = position
                    {
                        let origin = self.area.origin;
                        audio.break_clip(
                            clip,
                            std::array::from_fn(|i| origin[i] + f64::from(position[i])),
                        );
                    }
                    continue;
                }
            };
            // Source `SBroke`. A guest asks the host to break the item.
            if self.net_guest {
                if self.shared_breaks.len() < self.shared_breaks.capacity() {
                    self.shared_breaks.push((item, record as u32));
                }
                continue;
            }
            self.break_item(reference, record, item);
        }
        self.step_gibs(reference);
    }

    /// Source Break of item `item` with breakable record `record`: gib, clip and Destroy.
    pub(crate) fn break_item(
        &mut self,
        reference: &ReferenceManifest,
        record: usize,
        item: ItemId,
    ) {
        let rules = &reference.breakables[record];
        // Source Break calls pickupable.Drop2 first.
        if self.area.interaction.held() == Some(item) {
            self.area
                .interaction
                .drop_held(&mut self.area.items, &mut self.physics);
        }
        let Some((pose, buried)) = self
            .area
            .items
            .get(item)
            .filter(|s| !s.removed && s.location == crate::interaction::items::ItemLocation::World)
            .map(|s| (s.pose, s.buried))
        else {
            return;
        };
        self.spawn_gib(reference, rules, pose, buried);
        if let Some(clip) = self.stream.pick_break_clip(rules)
            && let Some(audio) = &mut self.audio
        {
            let origin = self.area.origin;
            audio.break_clip(
                clip,
                std::array::from_fn(|i| origin[i] + f64::from(pose.position[i])),
            );
        }
        if !rules.no_destroy && self.area.spawned.contains(item) {
            // A spawned copy leaves the spawned list, so its save and the tumbleweed list drop it.
            spawned::despawn(&mut self.area, &mut self.physics, item).expect("spawned copy");
        } else if !rules.no_destroy {
            self.stream
                .remove_broken_item(
                    reference,
                    record,
                    item,
                    &mut self.area.items,
                    &mut self.area.interaction.storage,
                    &mut self.physics,
                )
                .expect("bound storage");
        }
    }

    /// Source Break spawns Gib at the object pose. No reduced-gibs setting exists here.
    fn spawn_gib(
        &mut self,
        reference: &ReferenceManifest,
        rules: &crate::reference::BreakableReference,
        pose: crate::vehicle::VehiclePose,
        buried: bool,
    ) {
        let Some(gib) = rules.gib.as_ref().filter(|_| !rules.no_gib) else {
            return;
        };
        let prototype = reference
            .item_prototypes
            .iter()
            .position(|p| &p.id == gib)
            .expect("validated gib prototype");
        let gibs = spawned::spawn_prototype(
            reference,
            &mut self.area,
            &mut self.physics,
            prototype,
            pose,
        )
        .expect("validated gib prototype");
        // A buried object makes its gib buried.
        if buried {
            for gib in &gibs {
                crate::interaction::buried::bury(*gib, &mut self.area.items, &mut self.physics);
            }
        }
        self.area.gibs.spawn(prototype, gibs, pose);
        let origin = self.area.origin;
        self.gib_particles.spawn(
            prototype,
            std::array::from_fn(|i| origin[i] + f64::from(pose.position[i])),
            rapier3d::math::Rotation::from_array(pose.rotation),
        );
    }

    /// Gib starts after the breaks of this step, then gib updates and removals.
    fn step_gibs(&mut self, reference: &ReferenceManifest) {
        let player = self.area.player.body_pose(&self.physics).position;
        self.area.gibs.step(
            reference,
            self.stream.breakables.random(),
            player,
            &mut self.area.items,
            &mut self.physics,
            FIXED_DT,
        );
        let mut removed = std::mem::take(&mut self.area.gibs.removed);
        for item in removed.drain(..) {
            if self.area.spawned.contains(item) {
                spawned::despawn(&mut self.area, &mut self.physics, item).expect("spawned gib");
            }
        }
        self.area.gibs.removed = removed;
    }

    pub fn initialize_storage_frame(&mut self, dt: f32) -> Result<bool, ReferenceError> {
        let changed = self.area.interaction.storage.initialize_frame(
            &mut self.area.items,
            &mut self.physics,
            &self.area.interaction.parts,
            &self.area.interaction.mechanisms,
            self.area.player.item_seat_body(),
            if self.paused { 0.0 } else { dt },
        )?;
        if changed {
            self.wheels.sync(&mut self.area.items, &mut self.physics)?;
            self.area.items.sync_poses(&self.physics);
            self.area.commit_poses(&self.physics, true);
        }
        Ok(changed)
    }

    pub fn clear_input(&mut self) {
        self.stream.computers.typing = false;
        self.area.player.cancel_view_input();
        self.sleep.cancel_charge();
        self.clear_driving_input();
        self.area.interaction.cancel_input();
    }

    fn clear_driving_input(&mut self) {
        self.driving_input.clear();
        for vehicle in self.vehicles.values_mut() {
            vehicle.ignition.clear_input();
        }
        for part in self.powertrain.engines_mut() {
            part.state.clear_input();
        }
        self.area.interaction.cancel_input();
        self.area.inventory.cancel_input();
    }

    /// Moves the current vehicle, its cargo, and a walking player back to the road.
    pub fn recover(&mut self, reference: &ReferenceManifest) {
        if self.needs_output().dead {
            return;
        }
        self.interrupt_sleep(reference);
        self.audio_motion_revision = self.audio_motion_revision.wrapping_add(1);
        self.previous_speed = None;
        self.death_protected = true;
        let position = match &self.vehicle {
            Some(key) if self.physics.body_active(self.physics.chassis_handle(key)) => self
                .persistent_world
                .absolute_vehicle_position(&self.physics, &self.stream.origin, key),
            _ => self
                .stream
                .origin
                .absolute(self.area.player.focus_position(&self.physics)),
        };
        let empty = self
            .vehicle
            .as_ref()
            .and_then(|key| self.vehicles[key].fuel_tank)
            .and_then(|tank| self.area.items.get(tank))
            .and_then(|item| item.fluid.as_ref())
            .is_some_and(|fluid| fluid.total() <= 0.0);
        match self.stream.recovery_target(position, empty) {
            Ok(_) => {
                self.recovering = true;
                self.clear_input();
            }
            Err(error) => self.stream.fail(error),
        }
    }
    /// The engine part mounted on car `key` at the last step.
    fn mounted_engine(
        &self,
        key: &ObjectKey,
    ) -> Option<&crate::simulation::powertrain::EnginePart> {
        self.vehicles
            .get(key)?
            .engine()
            .and_then(|item| self.powertrain.engine(item))
    }
    /// Output of the engine part mounted on car `key`. Without an engine, the output is zero.
    pub fn engine_output(&self, key: &ObjectKey) -> EngineOutput {
        self.mounted_engine(key)
            .map_or_else(EngineOutput::default, |e| e.state.output())
    }
    /// Finds the parts and the fuel tank mounted on each car. The step reads the result.
    /// Part moves have no single change hook, so this runs once per step.
    pub fn refresh_powertrains(&mut self) {
        let items = &self.area.items;
        let parts = &self.area.interaction.parts;
        for (key, vehicle) in &mut self.vehicles {
            vehicle.refresh_parts(key, &self.powertrain, items, parts);
        }
    }
    /// Engine input without car controls: ambient temperature and clock rate.
    fn engine_climate(&self, reference: &ReferenceManifest) -> EngineInput {
        let globals = &reference.world.vehicle_globals;
        let ambient = self.lighting.cycle.output().ambient_temperature;
        let cycle = &reference.lighting.cycle;
        EngineInput {
            installed: None,
            ambient_temperature: ambient,
            ambient_heat_modifier: globals.outside_temperature_modifier
                * ((ambient - cycle.temp_nights[0])
                    / (cycle.temp_days[0] + cycle.temp_add_noons[0] - cycle.temp_nights[0]))
                    .clamp(0., 1.),
            sleep_modifier: self.clock_factors(reference).fluid_scale,
        }
    }
    /// Engines on no car step without car input while their body is active.
    /// Temperature moves toward ambient and rpm falls, as for a removed engine.
    fn step_loose_engines(&mut self, reference: &ReferenceManifest) {
        let climate = self.engine_climate(reference);
        let globals = &reference.world.vehicle_globals;
        for part in self.powertrain.engines_mut() {
            let on_car = self
                .vehicles
                .values()
                .any(|vehicle| vehicle.engine() == Some(part.item));
            let active = self
                .area
                .items
                .get(part.item)
                .and_then(|item| item.body)
                .is_some_and(|body| self.physics.body_active(body));
            if on_car || !active {
                continue;
            }
            part.state.step(
                climate,
                self.area.items.engine_fluids([None, part.oil_tank, None]),
                EngineRecords {
                    engine: &part.record,
                    radiator: None,
                    globals,
                },
                None,
                |draw| start_sample(DEFAULT_SEED, draw),
                FIXED_DT,
            );
            self.area
                .items
                .get_mut(part.item)
                .expect("engine item")
                .temperature = Some(part.state.temperature);
        }
    }
    /// Asks the stream for the area of the player focus. Returns the local focus.
    fn request_focus(&mut self) -> [f32; 3] {
        // An item seat moves the player with the item body, so the player stays the focus.
        let focus = if let Some(vehicle) = self.area.player.seat_vehicle() {
            self.physics.snapshot(vehicle).pose.position
        } else {
            self.area.player.focus_position(&self.physics)
        };
        self.request_world(self.stream.origin.absolute(focus));
        focus
    }
    /// Asks the stream for the area of `absolute` (the player), the cars that hold an area and the kept bodies.
    pub fn request_world(&mut self, absolute: [f64; 3]) {
        let car = self.vehicle.as_ref().map_or(absolute, |key| {
            self.persistent_world
                .absolute_vehicle_position(&self.physics, &self.stream.origin, key)
        });
        self.stream.request_at(absolute, car);
        let (world, physics, origin) = (&self.persistent_world, &self.physics, self.stream.origin);
        // Only the start car and the last seated car hold the area. Roadside cars follow the item rule.
        let last = self.vehicle.as_ref();
        let reference = std::sync::Arc::clone(self.stream.world_reference());
        let cars = self.vehicles.keys().filter(|key| {
            Some(*key) == last
                || matches!(key, ObjectKey::Generated(id) if reference.start_car_placement(id))
        });
        let cars = cars.map(|key| {
            (
                physics.chassis_handle(key),
                world.absolute_vehicle_position(physics, &origin, key),
            )
        });
        self.stream.request_cars(cars);
        let last_car = self
            .vehicle
            .as_ref()
            .map(|key| self.physics.chassis_handle(key));
        self.stream
            .request_kept([self.area.player.seat_body(), last_car]);
    }
    /// Moves the world origin near `focus` when it is far from the origin.
    pub fn rebase_to(&mut self, focus: [f32; 3]) {
        if self.stream.rebase(focus, &mut self.physics).is_some() {
            let old_origin = self.area.origin;
            self.area.sync_origin(self.stream.origin.offset);
            self.weather.set_origin(self.stream.origin.offset);
            let shift: [f32; 3] =
                std::array::from_fn(|i| (self.area.origin[i] - old_origin[i]) as f32);
            let shift_vector = rapier3d::math::Vector::from_array(shift);
            self.urine.rebase(shift_vector);
            self.flyers.rebase(shift_vector);
            self.creatures.rebase(shift_vector);
            if let Some(audio) = &mut self.audio {
                audio.rebase(shift);
            }
            self.aim.rebase(shift);
            self.rebases += 1;
        }
    }

    pub fn world_supported(&self) -> bool {
        if self.restore_pending()
            || !self.startup_aligned
            || !self.stream.initial_generation_ready()
        {
            return false;
        }
        // Source bodies return one sync after their own wider area loads. Wait for them too.
        let supported =
            |bounds| self.stream.supports(bounds) && !self.persistent_world.waits_for(bounds);
        // The stream loads its collision ranges before other work. The step waits for the same
        // areas, so a wait never depends on work that waits for the camera view.
        // A frozen car does not fall. The stream freezes a car whose ground it does not load.
        self.stream.collision_ranges().into_iter().all(supported)
            && self.vehicles.keys().all(|key| {
                let position = self.physics.snapshot(key).pose.position;
                let handle = self.physics.chassis_handle(key);
                !self.physics.body_active(handle)
                    || !self.physics.rapier().bodies[handle].is_dynamic()
                    || supported(crate::world::collision_window(
                        self.stream.origin.absolute(position),
                    ))
            })
    }
    /// Call after source publication, before deciding whether the simulation can advance.
    pub fn sync_world_activity(
        &mut self,
        reference: &ReferenceManifest,
    ) -> Result<(), crate::save::SaveError> {
        if self.startup_aligned {
            self.persistent_world.sync(
                reference,
                &mut self.area,
                &mut self.wheels,
                &self.stream,
                &mut self.physics,
            )?;
        }
        Ok(())
    }
    pub fn align_startup(&mut self, reference: &ReferenceManifest) -> Result<bool, ReferenceError> {
        if self.startup_aligned {
            return Ok(true);
        }
        let frame = self.stream.frame();
        let Some(startup) = frame.startup else {
            return Ok(false);
        };
        for (id, offset) in startup.body_offsets(reference) {
            let handle = self.area.bodies[&id];
            self.physics.edit_body(handle, |body| {
                let mut pose = *body.position();
                pose.translation.y += offset;
                body.set_position(pose, true);
            });
        }
        self.area
            .player
            .align_startup(startup.player_offset, &mut self.physics);
        let packs = cigarettes::start_pack_poses(
            reference,
            self.stream.start_car(),
            startup,
            self.area.origin,
        )?;
        for (pose, count) in packs {
            self.spawn_pack(reference, pose, count)?;
        }
        self.physics.refresh_queries();
        self.area.items.sync_poses(&self.physics);
        hose::spawn_start(reference, &mut self.area, &mut self.physics)?;
        self.area.commit_poses(&self.physics, true);
        self.startup_aligned = true;
        Ok(true)
    }
}

/// Source `carscript.InteractSwitchLights` of the world car whose switch usable is `switch`.
/// Returns the car.
fn use_light_switch(
    vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
    switch: ItemId,
) -> Option<(ObjectKey, usize)> {
    let (key, vehicle) = vehicles
        .iter_mut()
        .find(|(_, v)| v.light_switch == Some(switch))?;
    vehicle.cycle_headlights();
    Some((key.clone(), vehicle.prototype))
}

/// Car sound inputs of engine part `part`. None: the part has no sounds or no item.
fn engine_sound_input<'a>(
    part: &'a crate::simulation::powertrain::EnginePart,
    items: &crate::interaction::items::ItemStore,
    physics: &PhysicsWorld,
    mixer: Option<u32>,
) -> Option<crate::audio::cars::EngineSoundInput<'a>> {
    let audio = part.record.audio.as_ref()?;
    let item = items.get(part.item)?;
    let output = part.state.output();
    // A sleeping engine body is quiet.
    let active = item.body.is_some_and(|body| physics.body_active(body));
    Some(crate::audio::cars::EngineSoundInput {
        item: part.item,
        audio,
        max_rpm: part.record.max_rpm,
        start_time_modifier: part.record.engine_start_time_modifier,
        rpm: output.rpm,
        running: active && output.running,
        // Source `enginescript` skips the starter clip on a kick start engine.
        starter: active && output.starter && !part.record.kickstart,
        kick_start: active && output.kick_start,
        pose: to_rapier(item.pose),
        mixer,
    })
}

fn transport_body(
    physics: &mut PhysicsWorld,
    handle: RigidBodyHandle,
    delta: rapier3d::math::Pose,
) {
    let pose = delta * *physics.rapier().bodies[handle].position();
    physics.edit_body(handle, |body| {
        body.set_position(pose, true);
        body.set_linvel(rapier3d::math::Vector::ZERO, true);
        body.set_angvel(rapier3d::math::Vector::ZERO, true);
    });
}

pub(crate) fn start_sample(seed: u64, attempt: u64) -> f32 {
    (startup_hash(seed ^ attempt) >> 40) as f32 / 16_777_216.0
}

fn startup_hash(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

/// Longest drop that `rest_on_support` removes.
const MAX_REST_DROP: f32 = 0.3;
/// Gap left between a rested body and its support.
const REST_GAP: f32 = 0.005;

/// Moves each body straight down onto the first surface below it, at most `MAX_REST_DROP`.
/// A thin item that drops onto a thin board can spin through the top face in one step and stay
/// inside the board. The low wire brush starts 13 cm above its shelf board.
fn rest_on_support(physics: &mut PhysicsWorld, handles: &[RigidBodyHandle]) {
    let down = Vector::new(0.0, -MAX_REST_DROP, 0.0);
    let mut drops = Vec::with_capacity(handles.len());
    for &handle in handles {
        let body = &physics.rapier().bodies[handle];
        if !body.is_enabled() {
            continue;
        }
        let mut toi = 1.0f32;
        for &collider_handle in body.colliders() {
            let collider = &physics.rapier().colliders[collider_handle];
            if collider.is_sensor() || !collider.is_enabled() {
                continue;
            }
            let queries = physics.queries(
                QueryFilter::default()
                    .exclude_sensors()
                    .exclude_rigid_body(handle)
                    .groups(collider.collision_groups()),
            );
            if let Some((_, hit)) = queries.cast_shape(
                collider.position(),
                down,
                collider.shape(),
                ShapeCastOptions {
                    max_time_of_impact: 1.0,
                    stop_at_penetration: true,
                    ..Default::default()
                },
            ) {
                toi = toi.min(hit.time_of_impact);
            }
        }
        let drop = toi * MAX_REST_DROP - REST_GAP;
        if toi < 1.0 && drop > 0.0 {
            drops.push((handle, drop));
        }
    }
    for (handle, drop) in drops {
        physics.edit_body(handle, |body| {
            let mut pose = *body.position();
            pose.translation.y -= drop;
            body.set_position(pose, true);
        });
    }
    physics.refresh_queries();
}
