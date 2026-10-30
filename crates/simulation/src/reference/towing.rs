//! Source trailers (`utanfutoscript`, `attachgripscript`) and trailer hitch targets of world
//! prototypes. Every `u32` id is local to its world prototype: node, body or collider index.
use super::*;

/// Source `attachTargetscript` with tipus `utanfuto`. The grip snaps to `target`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldHitchReference {
    pub source: SourceId,
    /// The node of the script and its one collider.
    pub node: u32,
    /// Source `attachTargetscript.target`.
    pub target: u32,
    pub collider: u32,
    pub types: Vec<String>,
}

/// Source `utanfutoscript` on the root body of a trailer prototype.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldTrailerReference {
    pub source: SourceId,
    pub body: u32,
    pub wheels: Vec<SledWheelReference>,
    /// `wheelgraphicsscript.T` of each wheel: the node that `Graphics` poses from the wheel.
    pub graphics: Vec<u32>,
    /// `CenterOfMass` and `CenterOfMassAttached`, in the root frame.
    pub center_of_mass: [f32; 3],
    pub center_of_mass_hitched: [f32; 3],
    /// `brakeForce` on each wheel while the `brake` switch is on.
    pub brake_force: f32,
    /// `brake`: a `flashlightscript` switch of the trailer.
    pub brake: Option<SourceId>,
    pub grip: WorldGripReference,
    /// `rearlights` controllers. They follow the towing car.
    pub lights: Vec<WorldLightReference>,
}

/// Source `attachgripscript`: Start spawns `prototype` at `position` and joins it to the trailer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldGripReference {
    pub source: SourceId,
    /// The grip prefab root: a world prototype.
    pub prototype: SourceId,
    /// `VgripPos` (`fixGripPos`), in the root frame.
    pub position: [f32; 3],
    /// The grip `attachablescript.tipus`.
    pub types: Vec<String>,
}

impl WorldHitchReference {
    pub(super) fn valid(&self, p: &WorldPrototypeReference) -> bool {
        (self.node as usize) < p.nodes.len()
            && (self.target as usize) < p.nodes.len()
            && p.colliders
                .get(self.collider as usize)
                .is_some_and(|c| c.node == self.node)
            && !self.types.is_empty()
    }
}

impl WorldTrailerReference {
    pub(super) fn valid(&self, p: &WorldPrototypeReference) -> bool {
        finite(&self.center_of_mass)
            && finite(&self.center_of_mass_hitched)
            && finite(&self.grip.position)
            && self.brake_force.is_finite()
            && self.brake_force >= 0.
            && p.bodies
                .get(self.body as usize)
                .is_some_and(|b| b.node == 0)
            && !self.wheels.is_empty()
            && self
                .wheels
                .iter()
                .all(|w| (w.node as usize) < p.nodes.len() && w.collider.valid())
            && self.graphics.len() == self.wheels.len()
            && self
                .graphics
                .iter()
                .all(|&n| n != 0 && (n as usize) < p.nodes.len())
            && self.grip.prototype != p.source
            && !self.grip.types.is_empty()
            && self
                .lights
                .iter()
                .all(|l| l.kind == LightControllerKind::Rear && l.electronics.is_none())
    }
}
