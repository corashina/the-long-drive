//! Pumpkin light flicker (`lightdither`). It runs with the frame time, as the source does.
use super::lighting::{SourceLighting, dither_light};
use super::sim_frame::SimFrameResource;
use super::spawned::SpawnedVisuals;
use crate::app::RuntimeReference;
use crate::light_dither::LightDither;
use crate::reference::ItemId;
use bevy::prelude::*;

/// The item that owns the light and its dither state. A new owner runs `Start` again.
#[derive(Default)]
pub(crate) struct DitherOwner(Option<(ItemId, LightDither)>);

pub(crate) fn sync(
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    time: Res<Time>,
    spawned: Res<SpawnedVisuals>,
    mut owner: Local<DitherOwner>,
    mut lighting: ResMut<SourceLighting>,
) {
    let Some(record) = dither_light(&reference.0) else {
        return;
    };
    let Some(light) = lighting
        .lights
        .iter_mut()
        .find(|b| b.id == record.light.id && b.world.is_some())
    else {
        return;
    };
    let chosen = frame.frame.state.dither_light.checked_sub(1).map(ItemId);
    let node = chosen.and_then(|id| record.light.node.and_then(|n| spawned.node(id, n)));
    let world = light.world.as_mut().unwrap();
    let (Some(id), Some(node)) = (chosen, node) else {
        light.item = None;
        world.output.enabled = false;
        owner.0 = None;
        return;
    };
    if owner.0.as_ref().is_none_or(|(o, _)| *o != id) {
        owner.0 = Some((id, LightDither::new(record, id.0 as i32)));
    }
    let (_, state) = owner.0.as_mut().unwrap();
    light.item = Some(id);
    light.node = Some(node);
    world.output.enabled = record.light.enabled;
    world.output.intensity = state.step(record, time.delta_secs());
}
