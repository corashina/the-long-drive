//! Renderer data of one tick.
use super::GarageRuntime;
use crate::boundary::{
    BudgetRow, Change, CigaretteRow, ConditionRow, CreatureBoneRow, CreatureRow, PlaneRow, PoseRow,
    SimFrame, StateBlock, ValveRow,
};
use crate::creatures::animator::Action;
use crate::creatures::evil::EyeKind;
use crate::interaction::cigarettes::EmberPhase;
use crate::interaction::items::ItemLocation;
use crate::reference::{ItemId, ReferenceManifest};
use crate::vehicle::VehiclePose;
use crate::world::ObjectKey;

impl GarageRuntime {
    /// Writes this tick's poses and state into `frame`. `alpha` comes from the step clock.
    pub fn fill_frame(&mut self, reference: &ReferenceManifest, alpha: f32, frame: &mut SimFrame) {
        frame.changes.clear();
        let metrics = self.stream.metrics();
        let mut flags = 0;
        for (on, flag) in [
            (self.started, StateBlock::STARTED),
            (self.paused, StateBlock::PAUSED),
            (self.area.player.seated(), StateBlock::SEATED),
            (self.sleeping(), StateBlock::SLEEPING),
            (self.needs_output().dead, StateBlock::DEAD),
            (self.recovering, StateBlock::RECOVERING),
            (
                self.stream.frame().failure.is_some(),
                StateBlock::WORLD_FAILED,
            ),
            (
                self.stream.initial_generation_ready(),
                StateBlock::GENERATION_READY,
            ),
            (
                self.world_supported() && self.stream.required_ready(),
                StateBlock::WORLD_READY,
            ),
            (
                self.area.interaction.storage.initialization_ready(),
                StateBlock::STORAGE_READY,
            ),
            (
                metrics.generation_jobs == 0 && metrics.pending_work == 0,
                StateBlock::WORLD_IDLE,
            ),
        ] {
            if on {
                flags |= flag;
            }
        }
        let needs = self.needs_output();
        let dead_weight = if needs.dead {
            (needs.death_seconds as f32 / reference.survival.death_seconds).clamp(0., 1.)
        } else {
            0.
        };
        let area = &self.area;
        let stored_items = area.inventory.slots.map(|id| {
            id.filter(|id| {
                area.items
                    .get(*id)
                    .is_some_and(|i| matches!(i.location, ItemLocation::Stored(_)))
            })
            .map_or(0, |id| id.0 + 1)
        });
        let c = &area.cigarettes;
        let lit = |id: &ItemId| {
            c.cigarette(*id)
                .is_some_and(|s| s.ember.phase != EmberPhase::Start)
        };
        // One ember light: the worn cigarette, then the held one, then any lit one.
        let ember_light = area
            .wear
            .item(c.mouth_slot)
            .filter(lit)
            .or(area.interaction.held().filter(lit))
            .or_else(|| c.cigarettes.iter().map(|(id, _)| *id).find(lit));
        // One lightdither light: a worn pumpkin, then the held one, then the nearest in the world.
        let dithered = |id: &ItemId| {
            area.spawned.get(*id).is_some_and(|s| {
                reference.item_prototypes[s.prototype]
                    .light_dither
                    .is_some()
            })
        };
        let focus = rapier3d::math::Vector::from_array(area.player.focus_position(&self.physics));
        let dither_light = (0..crate::interaction::wear::WEAR_SLOTS)
            .filter_map(|slot| area.wear.item(slot))
            .find(dithered)
            .or(area.interaction.held().filter(dithered))
            .or_else(|| {
                area.spawned
                    .iter()
                    .map(|s| s.id)
                    .filter(dithered)
                    .filter_map(|id| {
                        let item = area.items.get(id)?;
                        matches!(item.location, ItemLocation::World).then(|| {
                            let d = rapier3d::math::Vector::from_array(item.pose.position) - focus;
                            (id, d.length_squared())
                        })
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(id, _)| id)
            });
        let anchor = self.sky_anchor(reference);
        let offset = self.stream.origin.offset;
        frame.state = StateBlock {
            simulation_seconds: self.simulation_seconds,
            origin: area.origin,
            stream_origin: offset,
            cycle_elapsed_seconds: self.lighting.cycle.elapsed_seconds(),
            session: self.stream.frame().request.session,
            selection_revision: self.stream.required_content().selection_revision,
            alpha,
            time_scale: self.clock_factors(reference).time_scale,
            flags,
            fov_degrees: area.player.view(&self.physics).fov_degrees,
            stored_items,
            body_pose: PoseRow::new(area.player.body_pose(&self.physics), 0),
            body_root: PoseRow::new(area.body.root_local(), 0),
            joint_scale: area.body.joint_scale(),
            sky_anchor: std::array::from_fn(|i| (anchor[i] - offset[i]) as f32),
            ember_light: ember_light.map_or(0, |id| id.0 + 1),
            dither_light: dither_light.map_or(0, |id| id.0 + 1),
            pad: 0,
            dead_weight,
            motion_revision: self.audio_motion_revision,
            far_plane: self.distances.cam2_far_plane,
            sleep_camera: self
                .sleep_camera()
                .map_or(PoseRow::default(), |camera| PoseRow {
                    position: self.stream.origin.local(camera.absolute_position),
                    flags: PoseRow::ACTIVE,
                    rotation: camera.rotation,
                }),
            // The driver sets the view camera.
            view_camera: PoseRow::default(),
            prepared_roads: metrics.prepared_roads as u32,
            glasses: crate::boundary::GlassesState::encode(area.wear.glasses),
            focus_distance: self.focus.distance,
            binocular_focus: 0.,
            // The driver sets the look and its own counters.
            look: crate::boundary::LookBlock::default(),
            perf: self.perf_block(),
        };
        // fpscontroller LateUpdate: the camera moves to the binocular `CamR` with its FOV.
        if let Some(b) = area.interaction.binoculars.active()
            && let Some(item) = area.items.get(b.item)
        {
            let camera = crate::vehicle::to_rapier(item.pose) * b.camera;
            frame.state.view_camera = PoseRow::new(
                VehiclePose {
                    position: camera.translation.to_array(),
                    rotation: area.player.view(&self.physics).camera_pose.rotation,
                },
                PoseRow::ACTIVE,
            );
            frame.state.fov_degrees = b.fov_degrees;
            frame.state.binocular_focus = b.focus;
            frame.state.flags |= StateBlock::BINOCULARS;
        }
        frame.valves.clear();
        frame.valves.extend(
            area.interaction
                .opening_valves()
                .map(|(id, valve)| ValveRow {
                    opening: id.0,
                    valve,
                }),
        );
        frame.cigarettes.clear();
        frame
            .cigarettes
            .extend(c.cigarettes.iter().map(|(id, state)| {
                let out = state.ember.output(&c.rates.ember);
                CigaretteRow {
                    item: id.0,
                    blend: state.part_blend(),
                    color: out.color,
                    light_intensity: out.light_intensity,
                }
            }));
        self.fill_particles(reference, frame);
        frame.decals.clear();
        frame.decals.extend(self.area.interaction.decals.rows());
        self.fill_conditions(reference, frame);
        self.fill_structures(reference, frame);
        self.fill_hud(reference, frame);
        self.fill_audio(reference, alpha, self.render_input, &mut frame.audio);
        frame.budgets.clear();
        if !self.started {
            let stream = &self.stream;
            frame.budgets.extend(
                stream
                    .presentation_budgets(stream.presentation_credit())
                    .map(|b| BudgetRow {
                        used: b.used as f64,
                        limit: b.limit as f64,
                    }),
            );
        }
        if let Some(aims) = self.home_cable(reference) {
            frame.changes.push(Change::HomeCable(aims));
        }
        self.screen_changes(frame);
        self.plate_changes(frame);
        frame.planes.clear();
        frame
            .planes
            .extend(self.sky().planes().iter().map(|plane| PlaneRow {
                id: plane.id,
                prefab: plane.prefab as u32,
                offset: plane.offset.to_array(),
                heading: plane.heading.to_array(),
            }));
        frame.creatures.clear();
        frame.creature_bones.clear();
        for v in self.creatures.views(&self.physics) {
            let start = frame.creature_bones.len() as u32;
            if v.dead {
                self.creatures.each_bone(&self.physics, v.id, |node, pose| {
                    frame.creature_bones.push(CreatureBoneRow {
                        node: node as u32,
                        position: pose.translation.to_array(),
                        rotation: pose.rotation.to_array(),
                    });
                });
            }
            let mut flags = 0;
            for (on, flag) in [
                (v.visible, PoseRow::VISIBLE),
                (v.dead, CreatureRow::DEAD),
                (!v.eyes, CreatureRow::NO_EYES),
                (v.evil == Some(EyeKind::Natural), CreatureRow::EVIL_EYES),
                (v.evil == Some(EyeKind::Green), CreatureRow::GREEN_EYES),
            ] {
                if on {
                    flags |= flag;
                }
            }
            frame.creatures.push(CreatureRow {
                id: v.id,
                flags,
                position: v.pose.translation.to_array(),
                rotation: v.pose.rotation.to_array(),
                scale: v.scale,
                phase: v.phase,
                value: v.value,
                twitch: v.twitch,
                action: v.action.map_or(CreatureRow::NO_ACTION, |a| match a.action {
                    Action::Roar => CreatureRow::ROAR,
                    Action::Attack => CreatureRow::ATTACK,
                }),
                action_time: v.action.map_or(0., |a| a.time),
                action_weight: v.action.map_or(0., |a| a.weight),
                bones: [start, frame.creature_bones.len() as u32 - start],
            });
        }
        let lighting = &self.lighting;
        frame.lighting.cycle.clone_from(lighting.cycle.output());
        frame.lighting.road_lamp = lighting.road_lamp_output(&reference.lighting);
        frame.lights.clear();
        frame.lights.extend_from_slice(lighting.light_outputs());
        frame.lamp_materials.clear();
        frame
            .lamp_materials
            .extend_from_slice(lighting.material_outputs());
        frame.control_poses.clear();
        frame
            .control_poses
            .extend_from_slice(lighting.control_poses());
        frame.node_visibility.clear();
        frame
            .node_visibility
            .extend_from_slice(lighting.visibility_outputs());
        self.frame_poses
            .fill(reference, &self.area, &self.physics, frame);
        frame.ragdoll.clear();
        let area = &self.area;
        if area.ragdoll.active() {
            frame
                .ragdoll
                .extend(reference.clothing.ragdoll.bodies.iter().map(|b| {
                    area.ragdoll.node_pose(&self.physics, b.node).map_or_else(
                        PoseRow::default,
                        |f| {
                            let pose = VehiclePose {
                                position: f.position.to_array(),
                                rotation: f.rotation.to_array(),
                            };
                            PoseRow::new(pose, PoseRow::VISIBLE)
                        },
                    )
                }));
        }
    }
    fn fill_conditions(&mut self, reference: &ReferenceManifest, frame: &mut SimFrame) {
        let store = &mut self.area.interaction.conditions;
        frame.conditions.clear();
        if self.conditions_sent {
            while let Some(id) = store.take_changed() {
                frame.conditions.push(ConditionRow::new(id, store.get(id)));
            }
        } else {
            while store.take_changed().is_some() {}
            frame
                .conditions
                .extend(store.iter().map(|(id, s)| ConditionRow::new(id, s)));
            self.conditions_sent = true;
        }
        frame.washes.clear();
        frame
            .washes
            .extend(store.washes.iter().map(|w| u8::from(w.visible())));
        let cleaning = &self.area.interaction.cleaning;
        frame.brush_poses.clear();
        frame.brush_poses.extend(
            reference
                .conditions
                .brushes
                .iter()
                .map(|b| PoseRow::new(cleaning.model_pose(b.item, &self.area.items), 0)),
        );
        frame.spray_colors.clear();
        frame.spray_colors.extend(
            reference
                .conditions
                .sprays
                .iter()
                .map(|t| cleaning.get(t.item).unwrap().color.unwrap()),
        );
    }
}

impl GarageRuntime {
    fn perf_block(&self) -> crate::boundary::PerfBlock {
        let counters = self.stream.performance_counters();
        crate::boundary::PerfBlock {
            render_cpu_bytes: counters.render_cpu_bytes as f64,
            render_gpu_bytes: counters.render_gpu_bytes as f64,
            contacts: self.vehicle.as_ref().map_or(0, |key| {
                self.physics.wheel_contacts(key).filter(|v| *v).count() as u32
            }),
            bodies: self.physics.rapier().bodies.len() as u32,
            colliders: self.physics.collider_count() as u32,
            roads: counters.roads as u32,
            terrain: counters.terrain as u32,
            objects: counters.objects as u32,
            jobs: counters.jobs as u32,
            pending: counters.pending as u32,
            storms: self.weather.storms().len() as u32,
            storm_strength: self.weather.influence().strength,
            weather_particles: self.weather.particle_count() as u32,
            ..Default::default()
        }
    }

    /// Sends the plate texts of each world car once.
    fn plate_changes(&mut self, frame: &mut SimFrame) {
        let vehicles = &self.vehicles;
        self.plates_sent
            .retain(|id| vehicles.contains_key(&ObjectKey::Generated(id.clone())));
        for (key, car) in vehicles {
            if let ObjectKey::Generated(id) = key
                && !car.plates.is_empty()
                && !self.plates_sent.contains(id)
            {
                self.plates_sent.push(id.clone());
                frame.changes.push(Change::Plates {
                    id: id.clone(),
                    texts: car.plates.clone(),
                });
            }
        }
    }
}

impl GarageRuntime {
    /// The simulation part of a world measurement.
    pub fn measurements(&self, reference: &ReferenceManifest) -> serde_json::Value {
        let frame = self.stream.frame();
        let car = self.vehicle.as_ref().map(|key| {
            let snapshot = self.physics.snapshot(key);
            serde_json::json!({
                "position": snapshot.pose.position, "rotation": snapshot.pose.rotation,
                "velocity": snapshot.linear_velocity, "angular_velocity": snapshot.angular_velocity,
                "wheel_contacts": self.physics.wheel_contacts(key).collect::<Vec<_>>(),
            })
        });
        let player = &self.area.player;
        serde_json::json!({
            "session": frame.request.session,
            "revision": frame.request.revision,
            "world": self.stream.metrics(),
            "simulation_seconds": self.simulation_seconds,
            "vehicles": self.vehicles.keys().map(|key| serde_json::json!({
                "key": key,
                "active": self.physics.body_active(self.physics.chassis_handle(key)),
                "absolute_position": self.persistent_world.absolute_vehicle_position(
                    &self.physics, &frame.origin, key),
            })).collect::<Vec<_>>(),
            "retained_source_bodies": self.area.bodies.iter().filter_map(|(id, handle)| {
                let record = reference.bodies.iter().find(|body| body.id == *id)?;
                self.persistent_world.body_snapshot(*handle, crate::save::BodyIdentity::Source {
                    source: crate::save::SourceKey::from(&record.source),
                })
            }).collect::<Vec<_>>(),
            "player": {
                "absolute_position": frame.origin.absolute(player.body_pose(&self.physics).position),
                "controller": player.body_pose(&self.physics).position,
                "body": self.physics.rapier().bodies[player.body_handle()].translation().to_array(),
                "grounded": player.grounded(),
            },
            "vehicle": car,
            "supported": self.world_supported(),
            "required_ready": self.stream.required_ready(),
            "failure": frame.failure,
            "gpu_ledger": {
                "prototype_gpu": self.stream.presentation_credit().prototype_gpu,
                "active_gpu": self.stream.presentation_credit().active_gpu,
            },
            "jobs": self.stream.startup_job_audit(),
            "physics_bodies": self.physics.rapier().bodies.len(),
            "physics_colliders": self.physics.collider_count(),
            "active_items": self.area.items.len(),
        })
    }
}
