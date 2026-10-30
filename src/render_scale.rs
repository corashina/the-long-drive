//! The scene scale setting as a Bevy resource.
use bevy::ecs as bevy_ecs;
pub(crate) use simulation::render_scale::SceneScale;

#[derive(bevy::prelude::Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneScaleSetting(pub SceneScale);
