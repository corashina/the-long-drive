//! One render-world view for each shadow tile bit of a source view.
//! Each tile view owns the binned caster phase of its bit, so equal casters batch in each tile.
//! Bevy preprocesses directional light views with their camera. The tile views use that path.
use super::surface_views::{SourceSurfaceView, WORLD_VIEWS};
use crate::reference::lamp_shadows::{LAMP_BIT, LAMP_SLOTS};
use bevy::pbr::{LightEntity, ViewLightEntities};
use bevy::render::view::{ExtractedView, NoIndirectDrawing, RetainedViewEntity};
use bevy::{ecs as bevy_ecs, prelude::*};

/// Caster mask bits: 4 sun cascades, then 4 lamp slots.
pub const TILE_BITS: usize = LAMP_BIT as usize + LAMP_SLOTS;

/// The tile views of one source view. Only the world view has lamp slots.
#[derive(Component)]
pub struct ShadowTileViews {
    parent: RetainedViewEntity,
    entities: [Entity; TILE_BITS],
    pub retained: [RetainedViewEntity; TILE_BITS],
    pub count: usize,
}

fn tile_view(parent: &ExtractedView, bit: usize) -> ExtractedView {
    let main = parent.retained_view_entity.main_entity;
    ExtractedView {
        retained_view_entity: RetainedViewEntity::new(main, Some(main), 100 + bit as u32),
        clip_from_view: parent.clip_from_view,
        world_from_view: parent.world_from_view,
        clip_from_world: parent.clip_from_world,
        target_format: parent.target_format,
        viewport: parent.viewport,
        color_grading: default(),
        invert_culling: false,
    }
}

/// Spawns the tile views once and lists them in the parent `ViewLightEntities`.
/// Bevy `prepare_lights` writes that list in `CreateViews`, so this runs after it.
pub(crate) fn prepare_tile_views(
    mut commands: Commands,
    mut views: Query<(
        Entity,
        &SourceSurfaceView,
        &ExtractedView,
        Option<&mut ShadowTileViews>,
        Option<&mut ViewLightEntities>,
    )>,
) {
    for (entity, source, view, tiles, lights) in &mut views {
        let entities = match tiles {
            Some(mut tiles) => {
                if tiles.parent != view.retained_view_entity {
                    tiles.parent = view.retained_view_entity;
                    for bit in 0..tiles.count {
                        let tile = tile_view(view, bit);
                        tiles.retained[bit] = tile.retained_view_entity;
                        commands.entity(tiles.entities[bit]).insert(tile);
                    }
                }
                tiles.entities
            }
            None => {
                let tiles = spawn_tiles(&mut commands, entity, source.index, view);
                let entities = tiles.entities;
                commands.entity(entity).insert(tiles);
                entities
            }
        };
        let count = tile_count(source.index);
        let entities = &entities[..count];
        // Link in the spawn frame too, so the first atlas draw has preprocessed instances.
        match lights {
            Some(mut lights) => {
                if !lights.lights.contains(&entities[0]) {
                    lights.lights.extend_from_slice(entities);
                }
            }
            None => {
                commands.entity(entity).insert(ViewLightEntities {
                    lights: entities.to_vec(),
                });
            }
        }
    }
}

fn tile_count(index: usize) -> usize {
    if index < WORLD_VIEWS {
        TILE_BITS
    } else {
        LAMP_BIT as usize
    }
}

fn spawn_tiles(
    commands: &mut Commands,
    parent: Entity,
    index: usize,
    view: &ExtractedView,
) -> ShadowTileViews {
    let count = tile_count(index);
    let mut entities = [Entity::PLACEHOLDER; TILE_BITS];
    let mut retained = [view.retained_view_entity; TILE_BITS];
    for bit in 0..count {
        let tile = tile_view(view, bit);
        retained[bit] = tile.retained_view_entity;
        entities[bit] = commands
            .spawn((
                tile,
                NoIndirectDrawing,
                LightEntity::Directional {
                    light_entity: parent,
                    cascade_index: bit,
                },
            ))
            .id();
    }
    ShadowTileViews {
        parent: view.retained_view_entity,
        entities,
        retained,
        count,
    }
}

pub fn install(app: &mut App) {
    use bevy::render::{Render, RenderApp, RenderSystems};
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        prepare_tile_views.in_set(RenderSystems::PrepareViews),
    );
}
