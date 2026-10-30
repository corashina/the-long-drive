//! Reused source pass order over Bevy's prepared transparent mesh items.
use super::{
    materials::MaterialLoading,
    readiness::RendererStateHandle,
    renderer_visibility::{CameraVisibility, SourceSortData},
    source_lighting::{DrawSourceAdd, DrawSourceBase, DrawSourceGlass},
};
use crate::reference::sorting::{PassSortKey, perspective_distance};
use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::render::{
    render_phase::{
        CachedRenderPipelinePhaseItem, DrawError, DrawFunctionId, DrawFunctions, PhaseItem,
        SortedRenderPhase, TrackedRenderPass, ViewSortedRenderPhases,
    },
    view::ExtractedView,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::ops::Range;

/// Bevy mesh phases retain main IDs and use a placeholder render entity.
#[derive(Resource, Default)]
pub struct SourceSortIndex(bevy::ecs::entity::EntityHashMap<SourceSortData>);
pub fn extract_sort_index(
    input: bevy::render::Extract<Res<CameraVisibility>>,
    mut index: ResMut<SourceSortIndex>,
) {
    index.0.clear();
    index.0.extend(
        input
            .sort_data
            .iter()
            .map(|(entity, data)| (*entity, *data)),
    );
}
#[derive(Clone, Copy)]
struct OrderedPass {
    key: PassSortKey,
    item: usize,
    draw: Option<DrawFunctionId>,
    cache_key: super::stained_glass::GrabPassKey,
    capture: bool,
    glass_source: i64,
    /// Named `_GrabTexture` user, or its capture when `draw` is None.
    grab: bool,
}
/// Allocate during view loading. The fixed scene cannot exceed this capacity.
#[derive(Component)]
pub struct SourceTransparentPasses {
    entries: Vec<OrderedPass>,
}
impl SourceTransparentPasses {
    pub fn new(maximum_passes: usize) -> Self {
        Self {
            entries: Vec::with_capacity(maximum_passes),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn capture_at(&self, index: usize) -> bool {
        self.entries[index].capture
    }
    pub fn named_at(&self, index: usize) -> bool {
        self.entries[index].grab
    }
    pub fn next_capture(&self, start: usize) -> usize {
        (start..self.entries.len())
            .find(|&i| self.entries[i].capture)
            .unwrap_or(self.entries.len())
    }
    pub fn glass_counts(&self) -> [u32; 4] {
        let mut counts = [0; 4];
        for entry in &self.entries {
            if entry.draw.is_none() && entry.grab {
                continue;
            } else if entry.draw.is_none() {
                counts[usize::from(!entry.capture)] += 1;
            } else if entry.glass_source == 47 {
                counts[2] += 1;
            } else if entry.glass_source == 48 {
                counts[3] += 1;
            }
        }
        counts
    }
    pub fn prepare_draws(world: &World) {
        world
            .resource::<DrawFunctions<Transparent3d>>()
            .write()
            .prepare(world);
    }
    /// End a range before each executed unnamed grab.
    pub fn render_range<'w>(
        &self,
        range: Range<usize>,
        phase: &SortedRenderPhase<Transparent3d>,
        pass: &mut TrackedRenderPass<'w>,
        world: &'w World,
        view: Entity,
    ) -> Result<(), DrawError> {
        let functions = world.resource::<DrawFunctions<Transparent3d>>();
        let mut functions = functions.write();
        for entry in &self.entries[range] {
            let Some(draw) = entry.draw else { continue };
            let item = phase
                .items
                .get_index(entry.item)
                .expect("prepared source pass index")
                .1;
            if !item.batch_range().is_empty() {
                functions
                    .get_mut(draw)
                    .expect("registered source pass")
                    .draw(world, pass, view, item)?;
            }
        }
        Ok(())
    }
}
pub(crate) fn prepare_pass_order(
    mut views: Query<(
        &ExtractedView,
        &mut SourceTransparentPasses,
        &super::surface_views::SourceSurfaceView,
    )>,
    frame: Res<super::view_frame::SourceViewFrame>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    metadata: Res<SourceSortIndex>,
    materials: Res<bevy::pbr::RenderMaterialInstances>,
    loading: Res<MaterialLoading>,
    functions: Res<DrawFunctions<Transparent3d>>,
    status: Res<RendererStateHandle>,
    particles: Res<super::particles::ExtractedParticles>,
    lighting: Res<super::lighting::ExtractedLighting>,
) {
    if !loading.is_ready() || status.failure().is_some() {
        for (_, mut order, _) in &mut views {
            order.entries.clear();
        }
        return;
    }
    let functions = functions.read();
    let base = functions.id::<DrawSourceBase>();
    let add = functions.id::<DrawSourceAdd>();
    let glass = functions.id::<DrawSourceGlass>();
    let particle_draw = functions.id::<super::particles::DrawSourceParticle>();
    for (view, mut order, source) in &mut views {
        order.entries.clear();
        if frame.skip_preparation(source.index) {
            continue;
        }
        // One named grab capture precedes the first distortion particle.
        let capacity = loading.transparent_pass_capacity().unwrap_or(0)
            + particles.uniforms.len()
            + crate::reference::GUN_DECALS
            + 1;
        if order.entries.capacity() < capacity {
            order.entries.reserve_exact(capacity);
        }
        let Some(phase) = phases.get(&view.retained_view_entity) else {
            continue;
        };
        let required = phase.items.len() * 2;
        if order.entries.capacity() < required {
            order.entries.reserve_exact(required);
        }
        let camera = view.world_from_view.translation().to_array();
        for (index, item) in phase.items.values().enumerate() {
            if item.batch_range().is_empty() || lighting.is_warmup(item.main_entity()) {
                continue;
            }
            if materials
                .instances
                .get(&item.main_entity())
                .is_some_and(|material| {
                    material.asset_id.type_id() == std::any::TypeId::of::<super::sky::SkyMaterial>()
                })
            {
                continue;
            }
            let Some(data) = metadata.0.get(&item.main_entity().id()) else {
                status.fail(format!(
                    "Source transparent sort metadata is missing: entity {:?}, material {:?}",
                    item.main_entity(),
                    materials
                        .instances
                        .get(&item.main_entity())
                        .map(|m| m.asset_id),
                ));
                return;
            };
            let TransparentSortingInfo3d::Sorted {
                mesh_center,
                depth_bias,
            } = item.sorting_info
            else {
                status.fail("Source transparent distance mode is invalid".into());
                return;
            };
            // Decals draw with the particle commands.
            let particle = materials
                .instances
                .get(&item.main_entity())
                .is_some_and(|m| {
                    let id = m.asset_id.type_id();
                    id == std::any::TypeId::of::<super::particles::ParticleMaterial>()
                        || id == std::any::TypeId::of::<super::decals::DecalMaterial>()
                });
            // Retain the particle adapter's bias in the current squared-distance order.
            let distance = perspective_distance(mesh_center.to_array(), camera)
                - if particle { depth_bias } else { 0. };
            if !distance.is_finite() || index > i32::MAX as usize {
                status.fail("Source transparent distance or gather index is invalid".into());
                return;
            }
            let count = 1 + usize::from(data.additive || data.glass);
            if order.entries.len() + count > order.entries.capacity() {
                status.fail("Source transparent pass capacity was exceeded".into());
                return;
            }
            let key = PassSortKey {
                layer_and_order: data.layer_and_order,
                queue: data.queue,
                distance,
                sorting_group: 0xfffff000,
                same_distance_priority: 0,
                node_index: index as i32,
                source_material_index: data.source_material_index,
                material_key: data.material_key,
                pass_data: 0x01000000,
                static_batch_first: 0,
                static_batch_count: 0,
                small_mesh_index: 0,
                lightmap_index: 0xffff,
                shader_identity: data.shader_identity,
                gather_index: index as i32,
            };
            // Target pipeline identity includes prepared raster state. Native runtime tie identities remain unobserved.
            let pass_identity = (item.cached_pipeline().id() as u64 + 1) << 2;
            let cache_key = super::stained_glass::GrabPassKey {
                material: u64::from(data.material_identity),
                pass: pass_identity,
                normalized_object_flags: 0,
                lightmap_index: 0xffff,
                light_mode: 0,
                instancing_keywords: 0,
                batching_key: 0,
                custom_properties_hash: 0,
            };
            if data.glass {
                order.entries.push(OrderedPass {
                    key: PassSortKey {
                        pass_data: key.pass_data | 0x04000000,
                        ..key
                    },
                    item: index,
                    draw: None,
                    cache_key,
                    capture: false,
                    glass_source: data.glass_source,
                    grab: false,
                });
            }
            order.entries.push(OrderedPass {
                key: if data.glass {
                    PassSortKey {
                        pass_data: 1,
                        ..key
                    }
                } else {
                    key
                },
                item: index,
                draw: Some(if particle {
                    particle_draw
                } else if materials
                    .instances
                    .get(&item.main_entity())
                    .is_some_and(|m| {
                        m.asset_id.type_id()
                            == std::any::TypeId::of::<super::plates::PlateTextMaterial>()
                    })
                {
                    item.draw_function()
                } else if data.glass {
                    glass
                } else {
                    base
                }),
                cache_key: super::stained_glass::GrabPassKey {
                    pass: pass_identity + u64::from(data.glass),
                    ..cache_key
                },
                capture: false,
                glass_source: data.glass_source,
                grab: data.grab,
            });
            if data.additive {
                order.entries.push(OrderedPass {
                    key: PassSortKey {
                        pass_data: 1,
                        ..key
                    },
                    item: index,
                    draw: Some(add),
                    cache_key: super::stained_glass::GrabPassKey {
                        pass: pass_identity + 1,
                        ..cache_key
                    },
                    capture: false,
                    glass_source: 0,
                    grab: false,
                });
            }
        }
        order.entries.sort_unstable_by(|a, b| a.key.compare(&b.key));
        let mut cache = super::stained_glass::GrabPassCache::default();
        for entry in &mut order.entries {
            let applied = cache.apply(entry.cache_key);
            entry.capture = applied && entry.draw.is_none();
        }
        cache.cleanup();
        // The source named GrabPass refreshes only at its first use in each camera render.
        if let Some(first) = order.entries.iter().position(|entry| entry.grab) {
            if order.entries.len() == order.entries.capacity() {
                status.fail("Source transparent pass capacity was exceeded".into());
                return;
            }
            let capture = OrderedPass {
                draw: None,
                capture: true,
                ..order.entries[first]
            };
            order.entries.insert(first, capture);
        }
    }
}
