//! Draw distances of one world session, resolved like `settingsscript.DrawDistanceChanged`.
use crate::reference::DrawDistanceReference;

macro_rules! distances {($($field:ident),* $(,)?) => {
    /// Resolved source `DDS` fields. Names follow `DrawDistanceSetting`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct DrawDistances {
        $(pub $field: f32,)*
    }
    impl DrawDistances {
        /// Clamp `value` to the source range, then interpolate each field in f32 like `Mathf`.
        pub fn resolve(r: &DrawDistanceReference, value: f32) -> Self {
            let (lo, hi) = (r.minimum.draw_distance, r.maximum.draw_distance);
            let value = value.clamp(lo, hi);
            let t = ((value - lo) / (hi - lo)).clamp(0., 1.);
            Self {
                $($field: r.minimum.$field + (r.maximum.$field - r.minimum.$field) * t,)*
            }
        }
        /// True when `value` resolves to these distances.
        pub fn matches(&self, r: &DrawDistanceReference, value: f32) -> bool {
            Self::resolve(r, value) == *self
        }
    }
}}
distances!(
    draw_distance,
    fog_modifier,
    cam1_far_plane,
    cam2_near_plane,
    cam2_far_plane,
    terrain_gen_dist,
    terrain_remove_dist,
    distant_terrain_gen_dist,
    distant_terrain_remove_dist,
    distant_terrain2_gen_dist,
    distant_terrain2_remove_dist,
    distant_terrain_shader1_near,
    distant_terrain_shader1_far,
    distant_terrain_shader1_amount,
    distant_terrain_shader2_near,
    distant_terrain_shader2_far,
    distant_terrain_shader2_amount,
    obj_gen_dist,
    obj_remove_dist,
    mountain_gen_dist,
    mountain_remove_dist,
    building_gen_dist,
    building_remove_dist,
    road_gen_dist,
    item_remove_dist,
    item_spawn_dist,
    item_freeze_dist,
    item_unfreeze_dist,
);
