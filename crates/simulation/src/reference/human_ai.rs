//! Source `newAiScript` AI fields of the human monster prefab.
//! Positions use the converted space. Angles are in degrees. Ranges are [min, max].
use super::*;

/// A capsule height and center of `colStand` or `colDuck`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapsuleShape {
    pub height: f32,
    pub center: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanAiReference {
    /// `sensU`, `sensH`, `sensD`, `sensG` and `sensCanGetUp` triggers, in their node space.
    pub sens: [LiveCollider; 5],
    /// `col`: the live capsule that lerps from `stand` to `duck`.
    pub col: NodeId,
    pub stand: CapsuleShape,
    pub duck: CapsuleShape,
    /// `lm` layer bits.
    pub mask: u32,
    pub fov: f32,
    pub range: f32,
    pub chase_time: [f32; 2],
    pub walk_dist: f32,
    pub run_dist: f32,
    pub target_dist: f32,
    pub target_dist_v: f32,
    pub min_speed: f32,
    pub max_speed: f32,
    pub max_velocity: f32,
    pub velocity_change: f32,
    pub rot_speed: f32,
    pub jump_velocity: f32,
    pub jump_velocity_duck: f32,
    pub duck_debounce_speed_up: f32,
    pub duck_debounce_speed_down: f32,
    pub grounded_vertical_velocity: f32,
    pub avoid_ray_x: u32,
    pub avoid_ray_y: u32,
    pub avoid_ray_range: f32,
    pub avoid_ray_vertical: f32,
    pub avoid_ray_angle: f32,
    pub avoid_force: f32,
    pub avoid_lerp: f32,
    pub ray_attack_dist_range: f32,
    /// `damage`. `Attack` takes `damage * dt` health per frame.
    pub damage: f32,
    pub door_speed: f32,
    pub start_patrol_dist: f32,
    pub patrol_ray_start_dist: f32,
    pub patrol_iteration: u32,
    pub patrol_ray_count: u32,
    pub patrol_ray_range: [f32; 2],
}

impl HumanAiReference {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let range = |r: &[f32; 2]| finite(r) && 0. <= r[0] && r[0] <= r[1];
        require(
            finite(&[
                self.fov,
                self.range,
                self.walk_dist,
                self.run_dist,
                self.target_dist,
                self.target_dist_v,
                self.min_speed,
                self.max_speed,
                self.max_velocity,
                self.velocity_change,
                self.rot_speed,
                self.jump_velocity,
                self.jump_velocity_duck,
                self.duck_debounce_speed_up,
                self.duck_debounce_speed_down,
                self.grounded_vertical_velocity,
                self.avoid_ray_range,
                self.avoid_ray_vertical,
                self.avoid_ray_angle,
                self.avoid_force,
                self.avoid_lerp,
                self.ray_attack_dist_range,
                self.damage,
                self.door_speed,
                self.start_patrol_dist,
                self.patrol_ray_start_dist,
                self.stand.height,
                self.duck.height,
            ]) && finite(&self.stand.center)
                && finite(&self.duck.center)
                && range(&self.chase_time)
                && range(&self.patrol_ray_range)
                && self.patrol_ray_count > 0,
            "human AI",
        )
    }
}
