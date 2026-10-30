//! Checked breakablescript records of selected world and item prototypes.
use super::*;

/// Prototype table of a breakable owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakableOwnerKind {
    /// `world.prototypes`. The node is a prototype node index.
    World,
    /// `item_prototypes`. The node is a manifest node id.
    Item,
}

/// One prototype node that holds the breakablescript.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakableOwner {
    pub kind: BreakableOwnerKind,
    pub prototype: SourceId,
    pub node: u32,
}

/// One breakchilds. It sends its force times its modifier to the breakable.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakableChild {
    pub transform: SourceId,
    /// False: breakablescript.Start sets the modifier to `child_modifier`.
    pub own_modifier: bool,
    pub modifier: f32,
}

/// The flashlightscript lamps that break with the breakable.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakableLamp {
    pub control: SourceId,
    /// Indices in `flashlightscript.Lamps`.
    pub lamps: Vec<u32>,
}

/// One breakablescript, as `Start` leaves it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakableReference {
    pub source: SourceId,
    pub owners: Vec<BreakableOwner>,
    pub health: f32,
    pub shoot_health: f32,
    pub shoot_different: bool,
    pub explosion_health: f32,
    pub explosion_different: bool,
    pub no_shoot: bool,
    pub decrease_after_attack: bool,
    pub only_decrease_above: bool,
    pub decrease_above_health: f32,
    pub only_velocity: bool,
    /// Never 0: breakablescript.Start changes 0 to 1.
    pub mass_modifier: f32,
    pub child_modifier: f32,
    pub children: Vec<BreakableChild>,
    pub no_trigger: bool,
    pub no_collider: bool,
    pub no_gib: bool,
    pub no_destroy: bool,
    /// Gib and reducedGib prototypes.
    pub gib: Option<PrototypeId>,
    pub reduced_gib: Option<PrototypeId>,
    pub clips: Vec<SourceId>,
    pub lamp: Option<BreakableLamp>,
}

/// mainscript values that gibscript.Start reads.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GibConstants {
    pub source: SourceId,
    /// gibMainChildsRandomRot, in degrees per axis.
    pub random_rotation: f32,
    /// gibMainChildsRandomPos, in metres.
    pub random_position: f32,
    /// gibMainChildsAddForce, a velocity change.
    pub add_force: f32,
}

impl GibConstants {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            self.source.file == "level1"
                && self.source.path_id == 64919
                && finite(&[self.random_rotation, self.random_position, self.add_force])
                && [self.random_rotation, self.random_position, self.add_force]
                    .iter()
                    .all(|v| *v >= 0.),
            "gib constants",
        )
    }
}

impl ReferenceManifest {
    /// The breakables record of a breakablescript.
    pub fn breakable(&self, source: &SourceId) -> Option<&BreakableReference> {
        self.breakables.iter().find(|b| &b.source == source)
    }
}

impl BreakableReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            finite(&[
                self.health,
                self.shoot_health,
                self.explosion_health,
                self.decrease_above_health,
                self.mass_modifier,
                self.child_modifier,
            ]) && self.mass_modifier != 0.
                && self.children.iter().all(|c| c.modifier.is_finite())
                && !self.owners.is_empty()
                && self
                    .clips
                    .iter()
                    .all(|c| item_prototypes::clip_exists(m, c)),
            "breakable values",
        )?;
        require(
            [&self.gib, &self.reduced_gib]
                .into_iter()
                .flatten()
                .all(|id| {
                    m.item_prototypes
                        .iter()
                        .any(|p| &p.id == id && p.item_id.is_none())
                }),
            "breakable gib prototype",
        )?;
        for owner in &self.owners {
            // Node sources of the owner prototype.
            let nodes: Vec<&SourceId> = match owner.kind {
                BreakableOwnerKind::World => {
                    let p = m
                        .world
                        .prototypes
                        .iter()
                        .find(|p| p.source == owner.prototype);
                    require(
                        p.is_some_and(|p| (owner.node as usize) < p.nodes.len()),
                        "breakable world owner",
                    )?;
                    p.unwrap().nodes.iter().map(|n| &n.source).collect()
                }
                BreakableOwnerKind::Item => {
                    let p = m.item_prototypes.iter().find(|p| p.id.0 == owner.prototype);
                    let asset = p.map(|p| p.asset);
                    require(
                        asset.is_some()
                            && m.nodes
                                .iter()
                                .any(|n| n.id.0 == owner.node && Some(n.asset) == asset),
                        "breakable item owner",
                    )?;
                    m.nodes
                        .iter()
                        .filter(|n| Some(n.asset) == asset)
                        .map(|n| &n.source)
                        .collect()
                }
            };
            require(
                self.children.iter().all(|c| nodes.contains(&&c.transform)),
                "breakable child owner",
            )?;
        }
        if let Some(lamp) = &self.lamp {
            let control = m
                .world
                .records
                .iter()
                .find(|r| r.source == lamp.control && r.class_name == "flashlightscript");
            let count = control
                .and_then(|r| r.fields.get("Lamps"))
                .and_then(|v| match v {
                    WorldValue::Array(lamps) => Some(lamps.len()),
                    _ => None,
                });
            // The runtime keeps the broken lamps of a control as u64 bits.
            require(
                count.is_some_and(|n| {
                    !lamp.lamps.is_empty()
                        && lamp
                            .lamps
                            .iter()
                            .all(|i| (*i as usize) < n && *i < u64::BITS)
                }),
                "breakable lamp control",
            )?;
        }
        Ok(())
    }
}
