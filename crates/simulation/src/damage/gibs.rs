//! Source gibscript and scaleDestroyScript on spawned gib items.
//! Start scatters the children of a main gib. Then a gib goes by time or by player distance.
//! The caller despawns the items in `removed`. Gibs are not saved.
use crate::interaction::items::ItemStore;
use crate::reference::{GibConstants, ItemId, ItemPrototype, NodeId, ReferenceManifest};
use crate::simulation::dials::unity_euler;
use crate::vehicle::{PhysicsWorld, VehiclePose, set_collider_enabled, to_rapier};
use crate::world::UnityRandom;
use rapier3d::math::{Rotation, Vector};

/// scaleDestroyScript.delayLength and scaleLength.
const SHRINK_DELAY: f32 = 1.;
const SHRINK_LENGTH: f32 = 3.;
/// scaleDestroyScript.random.
const SHRINK_RANDOM: f32 = 0.5;

/// gibscript.Update of a gib root that stays (not gibMain).
#[derive(Debug)]
struct Life {
    items: Vec<ItemId>,
    distance: bool,
    life: f32,
    /// Time.time - startTime.
    age: f32,
    colliders_off: bool,
    /// The root item. Without a root body the root stays at `position`.
    root: Option<ItemId>,
    position: [f32; 3],
}

#[derive(Debug, PartialEq)]
enum LifeEvent {
    Keep,
    CollidersOff,
    Remove,
}

impl Life {
    /// One gibscript.Update. `player_distance` is from the player to the gib root.
    fn update(&mut self, dt: f32, player_distance: f32) -> LifeEvent {
        let event = if self.distance {
            if player_distance >= self.life {
                LifeEvent::Remove
            } else {
                LifeEvent::Keep
            }
        } else if self.colliders_off {
            if self.age >= self.life {
                LifeEvent::Remove
            } else {
                LifeEvent::Keep
            }
        } else if self.age >= self.life * 0.95 {
            self.colliders_off = true;
            LifeEvent::CollidersOff
        } else {
            LifeEvent::Keep
        };
        self.age += dt;
        event
    }
}

/// scaleDestroyScript of one gib item.
#[derive(Debug)]
struct Shrink {
    item: ItemId,
    delay: f32,
    length: f32,
    age: f32,
    scale: f32,
    /// localScale of the node.
    shown: f32,
}

impl Shrink {
    /// One scaleDestroyScript.Update. True when the item goes.
    fn update(&mut self, dt: f32) -> bool {
        if self.age > self.delay + self.length {
            return true;
        }
        if self.age > self.delay {
            self.scale = self.scale.max(0.001);
            self.shown = self.scale;
            self.scale = (self.scale - dt).max(0.);
        }
        self.age += dt;
        false
    }
}

/// A gib spawned this step. Its Start runs in the next `step`.
struct Pending {
    prototype: usize,
    items: Vec<ItemId>,
    root: VehiclePose,
}

/// Spawned gibs with a gibscript or scaleDestroyScript.
#[derive(Default)]
pub struct Gibs {
    pending: Vec<Pending>,
    lives: Vec<Life>,
    shrinks: Vec<Shrink>,
    /// Items to despawn after `step`.
    pub removed: Vec<ItemId>,
}

impl Gibs {
    /// Source Instantiate(Gib). `items` are in prototype pickup order.
    pub fn spawn(&mut self, prototype: usize, items: Vec<ItemId>, root: VehiclePose) {
        self.pending.push(Pending {
            prototype,
            items,
            root,
        });
    }
    /// Shown scale of a shrinking item.
    pub fn scale(&self, item: ItemId) -> Option<f32> {
        self.shrinks
            .iter()
            .find(|s| s.item == item)
            .map(|s| s.shown)
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        for life in &mut self.lives {
            for (p, s) in life.position.iter_mut().zip(shift) {
                *p -= s;
            }
        }
    }
    /// Start of new gibs in spawn order, then Update of all gibs.
    pub fn step(
        &mut self,
        reference: &ReferenceManifest,
        random: &mut UnityRandom,
        player: [f32; 3],
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        dt: f32,
    ) {
        for pending in self.pending.drain(..) {
            let prototype = &reference.item_prototypes[pending.prototype];
            let item_at = |node: NodeId| item_at(reference, prototype, &pending.items, node);
            let root = item_at(prototype.root);
            let gib = prototype.behavior.gib.as_ref();
            if let Some(gib) = gib.filter(|g| g.gib_main) {
                start_main(
                    &reference.gib_constants,
                    gib,
                    &item_at,
                    random,
                    items,
                    physics,
                );
                // DetachChildren, then Destroy(gameObject).
                self.removed.extend(root);
            }
            for node in &prototype.behavior.scale_destroy {
                let delay = SHRINK_DELAY * random.range(1. - SHRINK_RANDOM, 1. + SHRINK_RANDOM);
                let length = SHRINK_LENGTH * random.range(1. - SHRINK_RANDOM, 1. + SHRINK_RANDOM);
                if let Some(item) = item_at(*node) {
                    self.shrinks.push(Shrink {
                        item,
                        delay,
                        length,
                        age: 0.,
                        scale: 1.,
                        shown: 1.,
                    });
                }
            }
            for piece in &prototype.behavior.gib_pieces {
                let mut piece_items = Vec::with_capacity(piece.nodes.len());
                for item in piece.nodes.iter().filter_map(|n| item_at(*n)) {
                    if !piece_items.contains(&item) {
                        piece_items.push(item);
                    }
                }
                self.lives.push(Life {
                    items: piece_items,
                    distance: piece.distance,
                    life: piece.life,
                    age: 0.,
                    colliders_off: false,
                    root: item_at(piece.node),
                    position: pending.root.position,
                });
            }
            if let Some(gib) = gib.filter(|g| !g.gib_main) {
                self.lives.push(Life {
                    items: pending.items,
                    distance: gib.distance,
                    life: gib.life,
                    age: 0.,
                    colliders_off: false,
                    root,
                    position: pending.root.position,
                });
            }
        }
        let removed = &mut self.removed;
        self.lives.retain_mut(|life| {
            let anchor = life
                .root
                .and_then(|id| items.get(id))
                .map_or(life.position, |s| s.pose.position);
            let distance = Vector::from_array(player).distance(Vector::from_array(anchor));
            match life.update(dt, distance) {
                LifeEvent::Keep => true,
                LifeEvent::CollidersOff => {
                    for id in &life.items {
                        disable_colliders(items, physics, *id);
                    }
                    true
                }
                LifeEvent::Remove => {
                    removed.extend_from_slice(&life.items);
                    false
                }
            }
        });
        self.shrinks.retain_mut(|shrink| {
            let gone = shrink.update(dt);
            if gone {
                removed.push(shrink.item);
            }
            !gone
        });
    }
}

/// The item of the nearest pickup body that holds `node`.
fn item_at(
    reference: &ReferenceManifest,
    prototype: &ItemPrototype,
    items: &[ItemId],
    node: NodeId,
) -> Option<ItemId> {
    let body = prototype.body_for(reference, Some(node))?;
    let pickup = prototype.pickups.iter().position(|p| p.body == body)?;
    items.get(pickup).copied()
}

/// gibscript.Start of a gibMain root. Children without an item still draw.
fn start_main(
    constants: &GibConstants,
    gib: &crate::reference::GibReference,
    item_at: &impl Fn(NodeId) -> Option<ItemId>,
    random: &mut UnityRandom,
    items: &mut ItemStore,
    physics: &mut PhysicsWorld,
) {
    let rotation = constants.random_rotation;
    let force = constants.add_force;
    let offset = constants.random_position;
    if !gib.dont_rot {
        for child in &gib.children {
            let item = item_at(*child);
            for _ in 0..2 {
                if random.range(0., 1.) < 0.5 {
                    let euler = [(); 3].map(|_| random.range(-rotation, rotation));
                    // Transform.Rotate in self space.
                    edit_pose(items, physics, item, |pose| {
                        pose.rotation = (Rotation::from_array(pose.rotation)
                            * Rotation::from_array(unity_euler(euler)))
                        .to_array();
                    });
                }
            }
            if random.range(0., 1.) < 0.5 {
                let axis = random.on_unit_sphere();
                let change = random.range(0., force);
                // AddTorque(VelocityChange). An axial vector converts as (-x, -y, z).
                let change = Vector::new(-axis[0], -axis[1], axis[2]) * change;
                add_velocity(items, physics, item, Vector::ZERO, change);
            }
        }
    }
    if !gib.add_small_pos {
        return;
    }
    let position = |items: &ItemStore, child: NodeId| {
        item_at(child)
            .and_then(|id| items.get(id))
            .map_or(Vector::ZERO, |s| Vector::from_array(s.pose.position))
    };
    let mut center = position(items, gib.children[0]);
    for child in &gib.children[1..] {
        center = (center + position(items, *child)) * 0.5;
    }
    for child in &gib.children {
        let item = item_at(*child);
        if random.range(0., 1.) < 0.5 {
            // Vector3.normalized is zero below 1e-5.
            let away = position(items, *child) - center;
            let away = if away.length() > 1e-5 {
                away.normalize()
            } else {
                Vector::ZERO
            };
            let away = away * random.range(0., offset);
            edit_pose(items, physics, item, |pose| {
                pose.position = (Vector::from_array(pose.position) + away).to_array();
            });
        }
        if random.range(0., 1.) < 0.5 {
            let move_by = unity_vector(random.on_unit_sphere()) * random.range(0., offset);
            edit_pose(items, physics, item, |pose| {
                pose.position = (Vector::from_array(pose.position) + move_by).to_array();
            });
        }
        if random.range(0., 1.) < 0.5 {
            let change = unity_vector(random.on_unit_sphere()) * random.range(0., force);
            add_velocity(items, physics, item, change, Vector::ZERO);
        }
    }
}

/// A source direction in physics space.
fn unity_vector(v: [f32; 3]) -> Vector {
    Vector::new(v[0], v[1], -v[2])
}

fn edit_pose(
    items: &mut ItemStore,
    physics: &mut PhysicsWorld,
    item: Option<ItemId>,
    edit: impl FnOnce(&mut VehiclePose),
) {
    let Some(state) = item.and_then(|id| items.get_mut(id)) else {
        return;
    };
    edit(&mut state.pose);
    let pose = to_rapier(state.pose);
    if let Some(body) = state.body {
        physics.edit_body(body, |b| b.set_position(pose, true));
    }
}

/// ForceMode.VelocityChange. Kinematic (buried) bodies ignore it.
fn add_velocity(
    items: &ItemStore,
    physics: &mut PhysicsWorld,
    item: Option<ItemId>,
    linear: Vector,
    angular: Vector,
) {
    let Some(body) = item.and_then(|id| items.get(id)).and_then(|s| s.body) else {
        return;
    };
    physics.edit_body(body, |b| {
        if b.is_dynamic() {
            b.set_linvel(b.linvel() + linear, true);
            b.set_angvel(b.angvel() + angular, true);
        }
    });
}

/// gibscript: `Collider.enabled = false` on all colliders of the gib.
fn disable_colliders(items: &ItemStore, physics: &mut PhysicsWorld, item: ItemId) {
    let Some(body) = items.get(item).and_then(|s| s.body) else {
        return;
    };
    let count = physics.rapier().bodies[body].colliders().len();
    for index in 0..count {
        let collider = physics.rapier().bodies[body].colliders()[index];
        physics.edit_collider(collider, |c| set_collider_enabled(c, false));
    }
    physics.refresh_queries();
}
