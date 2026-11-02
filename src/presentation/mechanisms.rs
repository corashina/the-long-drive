//! Bind driven nodes once. Body snapshots feed both render and collision poses.
use super::reference::{BodyVisual, ReferenceVisuals};
use crate::reference::{BodyRecord, ReferenceManifest};
use bevy::prelude::*;

pub fn bind_body(
    commands: &mut Commands,
    reference: &ReferenceManifest,
    visuals: &ReferenceVisuals,
    body: &BodyRecord,
) {
    let mut scale = Vec3::ONE;
    let mut current = Some(body.node);
    while let Some(id) = current {
        let node = reference
            .nodes
            .iter()
            .find(|node| node.id == id)
            .expect("validated node");
        scale *= Vec3::from_array(node.transform.scale);
        current = node.parent;
    }
    commands
        .entity(visuals.nodes[&body.node])
        .remove::<ChildOf>()
        .insert(BodyVisual {
            body: body.id,
            scale,
        });
}
