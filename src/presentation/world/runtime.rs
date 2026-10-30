//! Source publication order and shared presentation ownership.
use super::{
    SourceWorldVisuals, grass_jobs::GrassJobs, instance_materials::InstanceMaterials,
    instances::SourceInstances, pipelines::WorldPipelines, prototype_loader::SourceGltf,
    prototypes::SourcePrototypes,
};
use crate::{
    presentation::{
        lighting::SourceLighting,
        materials::{grass::writers::GrassWriters, terrain::TerrainMaterial},
    },
    reference::{ReferenceError, ReferenceManifest},
    world::RendererWorld,
};
use bevy::{
    ecs as bevy_ecs, ecs::system::SystemParam, mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
};
use grass_mesh::GrassView;
use simulation::boundary::{LampAdmission, WorldCommand};
#[derive(Resource, Default)]
pub struct SourcePresentationAllocation {
    session: u64,
    pub(crate) fixed: crate::ownership::fixed::FixedAllocation,
    pub(crate) lighting_cpu: usize,
    instance_cache_cpu: usize,
    lamps: (usize, usize, usize),
    imported_writers: Vec<(Entity, Handle<Mesh>)>,
    writers_bound: bool,
    /// Prototype indices of retired prototypes whose materials wait for release.
    retired_materials: Vec<usize>,
    /// Primary appearance assets of the contents, reused each frame.
    primary_assets: Vec<crate::reference::AssetId>,
    /// Renderer share of the world budgets.
    pub(crate) ledger: crate::ownership::PresentationLedger,
    /// Facts of the last renderer step. The world step applies them.
    facts: crate::world::RendererFacts,
    /// Renderer view of the stream. The world step updates it.
    pub(crate) world: Option<Box<RendererWorld>>,
    /// Admitted lamps of the last report to the world step.
    sent_lamps: Option<Vec<crate::world::GeneratedId>>,
}
impl SourcePresentationAllocation {
    /// Applies the world changes of a frame to the renderer copy.
    pub fn apply_world(
        &mut self,
        reference: &std::sync::Arc<crate::reference::WorldReference>,
        delta: &crate::world::WorldDelta,
    ) {
        self.world
            .get_or_insert_with(|| Box::new(RendererWorld::new(reference.clone())))
            .apply(delta);
    }
    pub fn bootstrap(
        session: u64,
        fixed: crate::ownership::fixed::FixedAllocation,
        ledger: crate::ownership::PresentationLedger,
    ) -> Self {
        Self {
            session,
            fixed,
            ledger,
            ..default()
        }
    }
    pub fn for_session(session: u64) -> Self {
        Self {
            session,
            ..default()
        }
    }
}
/// World object roots. The drive camera is excluded, so systems can also read its Transform.
pub type WorldRoots<'w, 's> =
    Query<'w, 's, &'static mut Transform, Without<crate::presentation::camera::DriveCamera>>;
#[derive(SystemParam)]
pub struct WorldPresentation<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub roots: WorldRoots<'w, 's>,
    pub prototypes: ResMut<'w, SourcePrototypes>,
    pub pipelines: ResMut<'w, WorldPipelines>,
    pub surfaces: ResMut<'w, SourceWorldVisuals>,
    pub instances: ResMut<'w, SourceInstances>,
    pub grass: ResMut<'w, GrassJobs>,
    pub writers: ResMut<'w, GrassWriters>,
    pub materials: InstanceMaterials<'w>,
    pub terrain: ResMut<'w, Assets<TerrainMaterial>>,
    pub assets: ResMut<'w, Assets<SourceGltf>>,
    pub plans: ResMut<'w, Assets<super::preflight::SourcePlan>>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub skins: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    pub batches: ResMut<'w, Assets<crate::presentation::texture_batches::SourceTextureBatch>>,
    pub chains: ResMut<'w, Assets<crate::presentation::primary_image::SourcePrimaryImage>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub server: Res<'w, AssetServer>,
    pub lights: ResMut<'w, SourceLighting>,
    pub imported: ResMut<'w, crate::presentation::reference::ReferenceVisuals>,
    pub fixed: ResMut<'w, crate::presentation::fixed_assets::SourceFixedAssets>,
    pub gltfs: Res<'w, Assets<bevy::gltf::Gltf>>,
    pub gltf_meshes: Res<'w, Assets<bevy::gltf::GltfMesh>>,
    pub shaders: Res<'w, Assets<Shader>>,
    pub dither: ResMut<'w, Assets<crate::presentation::lighting_assets::CasterDitherBytes>>,
    pub lighting_assets: ResMut<'w, crate::presentation::lighting_assets::LightingAssets>,
    pub imported_meshes: Query<'w, 's, (Entity, &'static ChildOf, &'static Mesh3d)>,
    pub world_poses: Query<'w, 's, &'static GlobalTransform>,
    pub allocation: ResMut<'w, SourcePresentationAllocation>,
    pub screens: ResMut<'w, crate::presentation::computer::ComputerScreens>,
}
impl WorldPresentation<'_, '_> {
    /// Applies the structure views of the simulation frame to the instances.
    pub(crate) fn sync_instances(
        &mut self,
        reference: &ReferenceManifest,
        sim: &mut crate::presentation::sim_frame::SimFrameResource,
    ) -> Result<(), ReferenceError> {
        self.screens
            .sync(&reference.computer.computer, sim, &mut self.images);
        let allocation = &mut *self.allocation;
        self.instances.sync(
            sim,
            reference,
            &mut self.commands,
            &mut self.roots,
            &mut self.materials,
            &self.screens,
            &mut allocation.ledger,
            allocation.world.as_deref(),
            &mut self.pipelines,
        )
    }
    /// Updates the renderer copy of `stream`.
    /// The stream origin of the renderer copy.
    pub fn origin(&self) -> Option<crate::world::WorldOrigin> {
        self.allocation.world.as_ref().map(|world| world.origin)
    }
    /// One renderer step on the copy.
    /// Adds the facts, the budget report and the admitted lamps of the step to `out`.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_frame(
        &mut self,
        reference: &ReferenceManifest,
        grant: Option<crate::world::PresentationGrant>,
        view: GrassView,
        time: f32,
        lod_view: super::lod::LodView,
        enabled: bool,
        light_outputs: &[crate::lighting::LightOutput],
        out: &mut WorldCommand,
    ) -> Result<(), ReferenceError> {
        if let Some(grant) = grant {
            self.allocation.ledger.grant(grant);
        }
        let world = self
            .allocation
            .world
            .take()
            .expect("renderer world synced before preparation");
        let result = self.prepare(
            &world,
            reference,
            view,
            time,
            lod_view,
            enabled,
            light_outputs,
        );
        let allocation = &mut *self.allocation;
        allocation.world = Some(world);
        out.facts.extend(std::mem::take(&mut allocation.facts));
        let report = allocation.ledger.report();
        out.report = Some(out.report.take().map_or(report, |r| r.merge(report)));
        let lamps = self.lights.road_lamps.admitted_lights();
        if lamps != allocation.sent_lamps.as_deref() {
            allocation.sent_lamps = lamps.map(<[_]>::to_vec);
            out.lamps = match lamps {
                None => LampAdmission::Pending,
                Some(lamps) => LampAdmission::Ready(lamps.to_vec()),
            };
        }
        result
    }
    /// One renderer step after the world step of the stream.
    fn prepare(
        &mut self,
        stream: &RendererWorld,
        reference: &ReferenceManifest,
        view: GrassView,
        time: f32,
        lod_view: super::lod::LodView,
        enabled: bool,
        light_outputs: &[crate::lighting::LightOutput],
    ) -> Result<(), ReferenceError> {
        let _profile =
            crate::app::cpu_profile::Timer::new(crate::app::cpu_profile::Stage::RenderPreparation);
        let mut timer =
            crate::world::stream_profile::Timer::new(crate::world::stream_profile::Stage::Cleanup);
        self.grass.set_view(view);
        let allocation = &mut *self.allocation;
        super::preflight::configure_staging(
            stream,
            &mut allocation.ledger,
            &mut allocation.facts,
            &reference.file_catalog,
        )?;
        let allocation = &mut *self.allocation;
        let mut failure = self
            .fixed
            .poll(
                &mut allocation.ledger,
                &mut allocation.fixed,
                reference,
                &self.server,
                &self.gltfs,
                &self.gltf_meshes,
                &self.shaders,
                &self.meshes,
                &mut self.images,
                &mut self.dither,
                &mut self.lights,
                &mut self.lighting_assets,
            )
            .err();
        if self.allocation.session != stream.frame().request.session {
            self.allocation.session = stream.frame().request.session;
            self.allocation.lamps = (0, 0, 0);
            self.allocation.writers_bound = false;
            self.allocation.imported_writers.clear();
            self.instances.retire(
                stream,
                &mut self.allocation.ledger,
                &mut self.commands,
                &mut self.meshes,
                &mut self.materials,
                &mut self.grass,
                &mut self.writers,
            )?;
            self.grass.release_domains(&mut self.allocation.ledger)?;
            self.materials.cache.begin_world_session();
            self.surfaces
                .retire_session(&mut self.commands, &mut self.meshes);
            self.surfaces.reset_bindings(&mut self.terrain);
        }
        // The world step published before this step. Published surfaces take their prepared renderer.
        self.surfaces.sync(
            &stream.frame(),
            &mut self.commands,
            &mut self.meshes,
            &mut self.roots,
        )?;
        // Cancellation and cleanup run before any material or stream failure can return.
        self.instances.retire(
            stream,
            &mut self.allocation.ledger,
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials,
            &mut self.grass,
            &mut self.writers,
        )?;
        self.surfaces
            .retire_stale(stream, &mut self.allocation.facts, &mut self.meshes)?;
        self.prototypes
            .retire_unused(stream, reference, &self.instances);
        let lighting_cpu = self.lights.clone_bytes();
        let mut controls = self.allocation.ledger.allocation();
        controls.active_cpu = controls
            .active_cpu
            .checked_sub(self.allocation.lighting_cpu)
            .and_then(|n| n.checked_add(lighting_cpu))
            .ok_or_else(|| ReferenceError("Fixed lighting control accounting".into()))?;
        self.allocation.ledger.set_allocation(controls)?;
        self.allocation.lighting_cpu = lighting_cpu;
        timer.next(crate::world::stream_profile::Stage::FixedInputs);
        let allocation = &mut *self.allocation;
        if let Err(error) = self.fixed.prepare_metadata(
            stream,
            &mut allocation.ledger,
            &mut allocation.fixed,
            reference,
            &self.server,
            &self.plans,
        ) {
            failure.get_or_insert(error);
        }
        if failure.is_some() || stream.frame().failure.is_some() {
            self.materials.cache.cancel_fixed();
        }
        if failure.is_none() && stream.frame().failure.is_none() && self.fixed.descriptors_ready() {
            let allocation = &mut *self.allocation;
            if let Err(error) = self.fixed.admit_payloads(
                &mut allocation.ledger,
                &mut allocation.fixed,
                reference,
                &self.server,
                &mut self.imported,
                &mut self.materials.cache,
                &mut self.lights,
                &mut self.lighting_assets,
                &mut self.images,
            ) {
                failure.get_or_insert(error);
            }
        }
        timer.next(crate::world::stream_profile::Stage::Pipelines);
        if let Err(error) = self.pipelines.prepare(stream, &mut self.allocation.ledger) {
            failure.get_or_insert(error);
        }
        self.writers
            .configure(stream, &mut self.allocation.ledger, &reference.world)?;
        self.writers.update_frame(
            view.position,
            Vec3::from_array(stream.origin.offset.map(|v| v as f32)),
            time,
        );
        timer.next(crate::world::stream_profile::Stage::PrimaryTextures);
        if let Err(error) = self.materials.cache.prepare_world_primary(
            stream,
            &mut self.allocation.ledger,
            &self.server,
            &mut self.images,
            &mut self.chains,
            &mut self.materials.primary,
        ) {
            failure.get_or_insert(error);
        }
        timer.next(crate::world::stream_profile::Stage::DeferredTextures);
        if let Err(error) = self.materials.cache.prepare_world_deferred(
            stream,
            &mut self.allocation.ledger,
            &self.server,
            &mut self.images,
            &mut self.batches,
        ) {
            failure.get_or_insert(error);
        }
        timer.next(crate::world::stream_profile::Stage::Prototypes);
        let allocation = &mut *self.allocation;
        if let Err(error) = self.prototypes.prepare(
            stream,
            &mut allocation.ledger,
            &mut allocation.facts,
            reference,
            &self.server,
            &mut self.assets,
            &mut self.plans,
            &mut self.meshes,
            &mut self.skins,
        ) {
            failure.get_or_insert(error);
        }
        timer.next(crate::world::stream_profile::Stage::MaterialRetirement);
        while let Some(source) = self.prototypes.take_retired() {
            let prototype = reference
                .world
                .prototypes
                .index(&source)
                .expect("retired prototype");
            self.allocation.retired_materials.push(prototype);
        }
        let mut index = 0;
        while index < self.allocation.retired_materials.len() {
            let prototype = &reference.world.prototypes[self.allocation.retired_materials[index]];
            if self.prototypes.requested(&prototype.source) {
                self.allocation.retired_materials.swap_remove(index);
                continue;
            }
            // Initial materials can name the asset of another prototype. That asset stays
            // while a live prototype still binds it.
            let own = !self.prototypes.references_asset(reference, prototype.asset);
            if (own
                && !self.materials.cache.world_asset_retirable(
                    prototype.asset,
                    &self.chains,
                    &self.server,
                ))
                || prototype.deferred_materials.iter().any(|m| {
                    !self.materials.cache.world_textures.retirable(
                        &m.source,
                        &self.images,
                        &self.server,
                    )
                })
                || prototype
                    .nodes
                    .iter()
                    .filter_map(|n| super::preflight::initial_material(reference, &n.source))
                    .any(|(asset, _)| {
                        !self.prototypes.references_asset(reference, asset)
                            && !self.materials.cache.world_asset_retirable(
                                asset,
                                &self.chains,
                                &self.server,
                            )
                    })
            {
                index += 1;
                continue;
            }
            if own {
                self.materials.cache.release_world_asset(
                    prototype.asset,
                    &mut self.images,
                    &mut self.chains,
                    &mut self.materials.primary,
                    &self.server,
                )?;
            }
            for (asset, _) in prototype
                .nodes
                .iter()
                .filter_map(|n| super::preflight::initial_material(reference, &n.source))
            {
                if !self.prototypes.references_asset(reference, asset) {
                    self.materials.cache.release_world_asset(
                        asset,
                        &mut self.images,
                        &mut self.chains,
                        &mut self.materials.primary,
                        &self.server,
                    )?;
                }
            }
            for material in &prototype.deferred_materials {
                if !self
                    .prototypes
                    .references_material(reference, &material.source)
                {
                    self.materials.cache.world_textures.release(
                        &material.source,
                        &mut self.images,
                        &self.server,
                    )?;
                }
            }
            self.allocation.retired_materials.swap_remove(index);
        }
        // Contents are read once, and only when a held painting asset could be released.
        let mut contents_read = false;
        for binding in &reference.world.painting_materials {
            if !self.materials.cache.holds_world_asset(binding.asset)
                || !self.materials.cache.world_asset_retirable(
                    binding.asset,
                    &self.chains,
                    &self.server,
                )
            {
                continue;
            }
            if !contents_read {
                stream.primary_assets(&mut self.allocation.primary_assets);
                contents_read = true;
            }
            if !self.allocation.primary_assets.contains(&binding.asset) {
                self.materials.cache.release_world_asset(
                    binding.asset,
                    &mut self.images,
                    &mut self.chains,
                    &mut self.materials.primary,
                    &self.server,
                )?;
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(error) = stream.frame().failure {
            return Err(ReferenceError(error.to_owned()));
        }
        timer.next(crate::world::stream_profile::Stage::Descriptors);
        let ledger = &mut self.allocation.ledger;
        self.surfaces
            .request_descriptors(stream, ledger, reference, &mut self.prototypes)?;
        self.instances
            .request_descriptors(stream, ledger, reference, &mut self.prototypes)?;
        // MaterialProperties resolves ShaderRef::Path during preparation.
        // Descriptor discovery stays available while fixed shader inputs wait.
        if !self.fixed.pipeline_inputs_ready() {
            return Ok(());
        }
        timer.next(crate::world::stream_profile::Stage::Bindings);
        let allocation = &mut *self.allocation;
        self.surfaces.prepare_bindings(
            stream,
            &mut allocation.ledger,
            &mut allocation.facts,
            reference,
            &mut self.prototypes,
            &self.server,
            &mut self.materials.cache,
            &mut self.materials.primary,
            &mut self.terrain,
        )?;
        timer.next(crate::world::stream_profile::Stage::Instances);
        let allocation = &mut *self.allocation;
        self.instances.prepare(
            stream,
            &mut allocation.ledger,
            &mut allocation.facts,
            reference,
            &mut self.prototypes,
            &self.server,
            &self.assets,
            &mut self.meshes,
            &mut self.materials,
            &mut self.grass,
            &mut self.pipelines,
            &self.fixed,
            &mut self.commands,
            view,
        )?;
        timer.next(crate::world::stream_profile::Stage::Lods);
        self.instances.select_lods(
            reference,
            lod_view,
            &mut self.commands,
            &mut self.materials,
            &mut self.grass,
        );
        timer.next(crate::world::stream_profile::Stage::Surface);
        let allocation = &mut *self.allocation;
        self.surfaces.prepare_surface(
            stream,
            &mut allocation.ledger,
            &mut allocation.facts,
            &mut self.meshes,
            &mut self.pipelines,
            &self.fixed,
        )?;
        timer.next(crate::world::stream_profile::Stage::Lamps);
        let lamps = super::lamps::source_lamp_admission(
            &stream.frame(),
            stream.required_lights(),
            &mut self.lights,
            reference,
            light_outputs,
            enabled,
        )?;
        let old = self.allocation.lamps;
        let mut a = self.allocation.ledger.allocation();
        a.active_cpu = a.active_cpu.saturating_sub(old.0) + lamps.0;
        a.active_gpu = a.active_gpu.saturating_sub(old.1) + lamps.1;
        a.objects = a.objects.saturating_sub(old.2) + lamps.2;
        self.allocation.ledger.set_allocation(a)?;
        self.allocation.lamps = lamps;
        timer.next(crate::world::stream_profile::Stage::InstanceSync);
        let instance_cache_cpu = self.instances.cache_bytes();
        if self.allocation.instance_cache_cpu != instance_cache_cpu {
            let mut allocation = self.allocation.ledger.allocation();
            allocation.active_cpu = allocation
                .active_cpu
                .checked_sub(self.allocation.instance_cache_cpu)
                .and_then(|bytes| bytes.checked_add(instance_cache_cpu))
                .ok_or_else(|| ReferenceError("Generated presentation cache accounting".into()))?;
            self.allocation.ledger.set_allocation(allocation)?;
            self.allocation.instance_cache_cpu = instance_cache_cpu;
        }
        timer.next(crate::world::stream_profile::Stage::LampSync);
        super::lamps::sync_source_lamps(
            &stream.frame(),
            reference,
            enabled,
            &mut self.lights,
            &mut self.commands,
            &mut self.roots,
        )?;
        timer.next(crate::world::stream_profile::Stage::Writers);
        self.instances.request_writers(
            &mut self.allocation.ledger,
            reference,
            &mut self.writers,
            &self.meshes,
        )?;
        if !self.allocation.writers_bound && self.imported.bound {
            for writer in &reference.world.grass_displacement.writers {
                if !writer.enabled {
                    continue;
                }
                let Some(node) = reference
                    .nodes
                    .iter()
                    .find(|n| n.source == writer.transform && n.active)
                else {
                    continue;
                };
                let Some(&entity) = self.imported.nodes.get(&node.id) else {
                    continue;
                };
                for (primitive, parent, mesh) in &self.imported_meshes {
                    if parent.parent() == entity {
                        self.allocation
                            .imported_writers
                            .push((primitive, mesh.0.clone()));
                    }
                }
            }
            let mut a = self.allocation.ledger.allocation();
            a.active_cpu += self.allocation.imported_writers.capacity()
                * std::mem::size_of::<(Entity, Handle<Mesh>)>();
            self.allocation.ledger.set_allocation(a)?;
            self.allocation.writers_bound = true;
        }
        let allocation = &mut *self.allocation;
        for (entity, mesh) in &allocation.imported_writers {
            if let Some(pose) = self
                .world_poses
                .get(*entity)
                .ok()
                .map(GlobalTransform::to_matrix)
            {
                self.writers.request(
                    &mut allocation.ledger,
                    *entity,
                    mesh.clone(),
                    pose,
                    &self.meshes,
                )?;
            }
        }
        self.writers
            .prepare(&mut self.allocation.ledger, &self.meshes)?;
        // Existing expansions must release their slots even when a new writer is pending.
        timer.next(crate::world::stream_profile::Stage::Grass);
        self.grass.prepare(
            stream,
            &mut self.allocation.ledger,
            self.writers.ready()?,
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials.grass,
            &mut self.pipelines,
            &self.fixed,
        )?;
        timer.next(crate::world::stream_profile::Stage::FinalCleanup);
        self.instances.retire(
            stream,
            &mut self.allocation.ledger,
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials,
            &mut self.grass,
            &mut self.writers,
        )?;
        self.prototypes
            .retire_unused(stream, reference, &self.instances);
        Ok(())
    }
}
