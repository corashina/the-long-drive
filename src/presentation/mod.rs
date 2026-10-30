pub mod ambient_occlusion;
pub mod antialiasing;
pub mod camera;
pub mod camera_projection;
mod cap_motion;
pub mod cigarettes;
pub mod computer;
pub mod conditions;
pub mod consumables;
pub mod creatures;
pub mod decals;
pub mod depth_of_field;
pub mod figure_stage;
pub mod fixed_assets;
pub mod gpu_prepare;
pub mod gpu_timing;
pub mod lamp_shadows;
pub mod light_dither;
pub mod lighting;
pub mod materials;
pub mod mechanisms;
pub mod menu_figure;
mod menu_ik;
pub mod mirrors;
pub mod motion_blur;
pub mod motion_vectors;
pub mod outfit;
pub mod outfit_preview;
pub mod player_body;
pub mod ragdoll;
pub mod reference;
pub mod reflections;
pub mod remote_players;
pub mod screen_effects;
mod shadow_batch;
mod shadow_cache;
pub mod shadow_quality;
pub(crate) mod sim_frame;
pub mod sky_traffic;
pub mod spawned;
pub mod sunglasses;
pub mod temporal;
pub mod world;
pub mod world_metrics;

use crate::vehicle::VehiclePose;
use bevy::prelude::*;

pub fn transform(pose: VehiclePose) -> Transform {
    Transform::from_translation(Vec3::from_array(pose.position))
        .with_rotation(Quat::from_array(pose.rotation))
}

pub use simulation::driver::interpolate;
pub mod texture_batches;
pub mod textures;

pub mod sky;

pub mod caster;
pub(crate) mod caster_material;
pub mod clouds;
pub mod depth_resolve;
pub mod frame_copy;
pub mod frame_graph;
pub mod lighting_assets;
pub mod lighting_scene;
pub mod particles;
pub mod plates;
pub mod postprocessing;
pub mod probes;
pub mod prop_draws;
pub mod render_bundles;
pub mod render_targets;
pub mod renderer_visibility;
pub mod renderers;
pub mod shadow_atlas;
mod shadow_changes;
pub mod shadow_collector;
pub mod shadow_phase;
pub(crate) mod shadow_preparation;
pub mod shadow_queue;
pub mod shadow_scene;
pub mod source_effects;
pub mod source_images;
pub(crate) mod source_io;
pub mod source_lighting;
pub mod source_sky;
pub mod stained_glass;
pub mod static_draws;
pub mod surface_graph;
pub mod surface_pass;
pub mod surface_views;
pub mod transparent;
pub mod view_frame;

mod pipeline_ownership;

pub mod primary_image;

pub(crate) mod readiness;
