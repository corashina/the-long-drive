//! Checked portable item bindings.
use super::{
    ColliderId, ItemId, LightControlId, LightControllerKind, NodeId, ReferenceError,
    ReferenceManifest, SourceId, require,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashlightRecord {
    pub models: Vec<FlashlightModelReference>,
    pub instances: Vec<FlashlightInstanceReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashlightModelReference {
    pub source: SourceId,
    pub controller: SourceId,
    pub node: NodeId,
    pub meshes: Vec<SourceId>,
    pub materials: Vec<SourceId>,
    pub renderers: Vec<SourceId>,
    pub lights: Vec<SourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashlightInstanceReference {
    pub model: SourceId,
    pub item: ItemId,
    pub node: NodeId,
    pub pickup: SourceId,
    pub save: SourceId,
    pub controller: LightControlId,
    pub startup_active: bool,
    pub colliders: Vec<ColliderId>,
}

impl FlashlightRecord {
    pub(super) fn validate(&self, manifest: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.models.len() == 2
                && self.instances.len() == 2
                && self
                    .instances
                    .iter()
                    .filter(|item| item.startup_active)
                    .count()
                    == 1,
            "flashlight model and startup count",
        )?;
        for (model, instance) in self.models.iter().zip(&self.instances) {
            require(
                model.source == instance.model
                    && model.node == instance.node
                    && !model.meshes.is_empty()
                    && !model.materials.is_empty()
                    && !model.renderers.is_empty()
                    && !model.lights.is_empty(),
                "flashlight model binding",
            )?;
            let item = manifest.items.iter().find(|item| item.id == instance.item);
            let node = manifest.nodes.iter().find(|node| node.id == instance.node);
            let body =
                item.and_then(|item| manifest.bodies.iter().find(|body| body.id == item.body));
            let control = manifest
                .lighting
                .controllers
                .iter()
                .find(|control| control.id == instance.controller);
            require(
                item.is_some_and(|item| item.node == instance.node && item.pickup.is_some())
                    && node.is_some_and(|node| node.active == instance.startup_active)
                    && body.is_some_and(|body| body.dynamic && body.mass_kg == 1.0)
                    && control.is_some_and(|control| {
                        control.node == instance.node
                            && control.kind == LightControllerKind::Switch
                            && !control.startup_on
                            && control.enabled == instance.startup_active
                            && control.states.len() == 1
                    })
                    && !instance.colliders.is_empty()
                    && instance.colliders.iter().all(|id| {
                        manifest.colliders.iter().any(|collider| {
                            collider.id == *id && collider.enabled == instance.startup_active
                        })
                    }),
                "flashlight item, body, controller, or collider",
            )?;
        }
        Ok(())
    }
}
