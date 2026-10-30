//! The "Depth of field" display setting (source `BDof`). Off disables the `DOF` volume. Binoculars keep their DoF.
use bevy::{ecs as bevy_ecs, prelude::*};

/// Source `ToggleDepthOfField.def` is off.
#[derive(Resource, Default)]
pub struct DepthOfField(pub bool);

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut setting: ResMut<DepthOfField>) {
    if let Some(on) = crate::browser::game_depth_of_field() {
        setting.0 = on;
    }
}
