use bevy::math::Affine3A;
use bevy::prelude::{Transform, Vec3};

pub(super) fn update_needle(
    transform: &mut Transform,
    start_world: Affine3A,
    end_world: Affine3A,
    parent_world: Affine3A,
    tuning: f32,
) {
    let point: Vec3 = start_world
        .translation
        .lerp(end_world.translation, tuning.clamp(0., 1.))
        .into();
    let translation = parent_world.inverse().transform_point3(point);
    if transform.translation != translation {
        transform.translation = translation;
    }
}
