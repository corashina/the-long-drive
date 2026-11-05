//! Source `mpplayerscript` colliders of one remote player and its `massScript` rigidbody.
use crate::reference::RemotePlayerReference;
use crate::vehicle::{PhysicsWorld, set_collider_enabled};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::intersection_test;
use rapier3d::prelude::{
    CoefficientCombineRule, ColliderBuilder, ColliderHandle, Group, InteractionGroups,
    InteractionTestMode, RigidBodyBuilder, RigidBodyHandle, RigidBodyType, SharedShape,
};

/// Source `Start`. The colliders turn on 3 s after the figure shows.
const SPAWN_DELAY: f64 = 3.;
/// Source `UpdCollider`. The colliders turn on 1 s after the figure stands up.
const STAND_DELAY: f64 = 1.;
/// Unity default material of the head colliders.
const DEFAULT_FRICTION: f32 = 0.6;

/// `UpdCollider` geometry in the figure frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shapes {
    pub body_center: Vector,
    pub body_half: f32,
    pub radius: f32,
    pub head: Vector,
}

impl Shapes {
    /// `head` is `THead` in the figure frame. The capsule stands on `TProneHeadPos` and reaches head
    /// height. The figure stands upright, so its frame Y is world Y.
    pub fn new(r: &RemotePlayerReference, head: Vector, duck: f32) -> Self {
        let prone = Vector::from_array(r.prone_head);
        let top = Vector::new(prone.x, head.y, prone.z);
        let radius = r.minimum_radius + (r.maximum_radius - r.minimum_radius) * duck.clamp(0., 1.);
        // Unity clamps the capsule height to at least its diameter.
        let body_half = ((top - prone).length() * 0.5 - radius).max(0.);
        Self {
            body_center: (top + prone) * 0.5,
            body_half,
            radius,
            head,
        }
    }
}

pub struct RemoteBody {
    pub handle: RigidBodyHandle,
    /// `StandBody`, the head sphere, the head box, `Feet2`.
    colliders: [ColliderHandle; 4],
    head_offsets: [Vector; 2],
    enable_at: f64,
    enabled: bool,
    seated: bool,
    /// Bodies that this body ignores: the items its player holds.
    ignored: Vec<RigidBodyHandle>,
}

fn groups(membership: u32, filter: u32) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(membership),
        Group::from_bits_retain(filter),
        InteractionTestMode::And,
    )
}

impl RemoteBody {
    /// Source `Start` and `massScript.AddRB`: a 75 kg body without gravity or rotation.
    pub fn new(
        r: &RemotePlayerReference,
        shapes: Shapes,
        pose: Pose,
        now: f64,
        physics: &mut PhysicsWorld,
    ) -> Self {
        let handle = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .pose(pose)
                .gravity_scale(0.)
                .lock_rotations()
                .ccd_enabled(true)
                .additional_mass(r.mass_kg),
        );
        let standing = groups(r.standing_membership, r.standing_filter);
        let slide = |b: ColliderBuilder| {
            b.friction(r.friction)
                .friction_combine_rule(CoefficientCombineRule::Min)
                .density(0.)
                .collision_groups(standing)
        };
        let plain = |b: ColliderBuilder| {
            b.friction(DEFAULT_FRICTION)
                .density(0.)
                .collision_groups(standing)
        };
        let head_offsets = [Vector::ZERO, Vector::from_array(r.head_box_center)];
        let builders = [
            slide(ColliderBuilder::capsule_y(shapes.body_half, shapes.radius)),
            plain(ColliderBuilder::ball(r.head_radius)),
            plain(ColliderBuilder::cuboid(
                r.head_box_half[0],
                r.head_box_half[1],
                r.head_box_half[2],
            )),
            slide(
                ColliderBuilder::ball(r.feet_radius).translation(Vector::from_array(r.feet_center)),
            ),
        ];
        let colliders = builders.map(|b| physics.insert_collider_with_parent(b.build(), handle));
        let mut body = Self {
            handle,
            colliders,
            head_offsets,
            enable_at: now + SPAWN_DELAY,
            enabled: true,
            seated: false,
            ignored: Vec::new(),
        };
        body.set_enabled(physics, false);
        body.set_shapes(physics, shapes);
        physics.recompute_body_mass(handle);
        body
    }

    /// Source `UpdCollider` on each received pose.
    pub fn update(
        &mut self,
        r: &RemotePlayerReference,
        shapes: Shapes,
        seated: bool,
        now: f64,
        physics: &mut PhysicsWorld,
    ) {
        if self.seated && !seated {
            self.set_enabled(physics, false);
            self.enable_at = now + STAND_DELAY;
        }
        if seated != self.seated {
            // Source `massScript.RemoveRB` while seated. The colliders stop being a dynamic body.
            let kind = if seated {
                RigidBodyType::KinematicPositionBased
            } else {
                RigidBodyType::Dynamic
            };
            physics.edit_body(self.handle, |b| b.set_body_type(kind, true));
            let (membership, filter) = if seated {
                (r.seated_membership, r.seated_filter)
            } else {
                (r.standing_membership, r.standing_filter)
            };
            for c in self.colliders {
                physics.edit_collider(c, |c| c.set_collision_groups(groups(membership, filter)));
            }
            self.seated = seated;
            self.apply(physics);
        }
        if !seated {
            physics.edit_body(self.handle, |b| {
                b.set_linvel(Vector::ZERO, false);
                b.set_angvel(Vector::ZERO, false);
            });
        }
        self.set_shapes(physics, shapes);
    }

    /// Source `EnableCollidersWithDelay`. A standing figure waits while it overlaps the body of
    /// `player`, so a spawn on a player never pushes the player out.
    pub fn tick(&mut self, now: f64, physics: &mut PhysicsWorld, player: RigidBodyHandle) {
        if !self.enabled
            && now >= self.enable_at
            && (self.seated || !self.overlaps(physics, player))
        {
            self.set_enabled(physics, true);
        }
    }

    /// The figure colliders touch an enabled solid collider of `other`.
    fn overlaps(&self, physics: &PhysicsWorld, other: RigidBodyHandle) -> bool {
        let rapier = physics.rapier();
        let (Some(own), Some(other)) = (rapier.bodies.get(self.handle), rapier.bodies.get(other))
        else {
            return false;
        };
        self.colliders.iter().any(|&a| {
            let a = &rapier.colliders[a];
            let pa = *own.position() * a.position_wrt_parent().copied().unwrap_or_default();
            other.colliders().iter().any(|&b| {
                let b = &rapier.colliders[b];
                b.is_enabled()
                    && !b.is_sensor()
                    && intersection_test(
                        &pa,
                        a.shape(),
                        &(*other.position() * b.position_wrt_parent().copied().unwrap_or_default()),
                        b.shape(),
                    )
                    .is_ok_and(|hit| hit.intersecting)
            })
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn seated(&self) -> bool {
        self.seated
    }

    /// The body position. Contacts move a standing figure between writes.
    pub fn position(&self, physics: &PhysicsWorld) -> Vector {
        physics.rapier().bodies[self.handle].translation()
    }

    /// The figure Transform `T`.
    pub fn place(&self, physics: &mut PhysicsWorld, pose: Pose) {
        physics.edit_body(self.handle, |b| {
            if b.is_kinematic() {
                b.set_next_kinematic_position(pose);
            } else {
                b.set_position(pose, true);
            }
        });
    }

    /// Source `Physics.IgnoreCollision` with each item of `items`, and with no other body.
    pub fn ignore_only(&mut self, physics: &mut PhysicsWorld, items: &[RigidBodyHandle]) {
        for &b in &self.ignored {
            if !items.contains(&b) {
                physics.ignore_pair(self.handle, b, false);
            }
        }
        for &b in items {
            if !self.ignored.contains(&b) {
                physics.ignore_pair(self.handle, b, true);
            }
        }
        self.ignored.clear();
        self.ignored.extend_from_slice(items);
    }

    fn set_enabled(&mut self, physics: &mut PhysicsWorld, enabled: bool) {
        self.enabled = enabled;
        self.apply(physics);
    }

    /// In the source a seated figure's colliders join the car. The port keeps them for queries only,
    /// so the kinematic body does not push the car it sits in.
    fn apply(&self, physics: &mut PhysicsWorld) {
        let solid = self.enabled && !self.seated;
        for c in self.colliders {
            physics.edit_collider(c, |c| {
                set_collider_enabled(c, self.enabled);
                if !solid {
                    c.set_solver_groups(InteractionGroups::none());
                }
            });
        }
    }

    fn set_shapes(&self, physics: &mut PhysicsWorld, s: Shapes) {
        let [body, sphere, cube, _] = self.colliders;
        physics.edit_collider(body, |c| {
            c.set_shape(SharedShape::capsule_y(s.body_half, s.radius));
            c.set_position_wrt_parent(Pose::from_parts(s.body_center, Rotation::IDENTITY));
        });
        for (c, offset) in [sphere, cube].into_iter().zip(self.head_offsets) {
            physics.edit_collider(c, |c| {
                c.set_position_wrt_parent(Pose::from_translation(s.head + offset))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{ManagedEvidence, NodeId, SourceId};

    fn reference() -> RemotePlayerReference {
        RemotePlayerReference {
            source: SourceId {
                file: "sharedassets0.assets".into(),
                path_id: 1389,
            },
            head: NodeId::default(),
            prone_head: [0., -0.685, 0.],
            minimum_radius: 0.1,
            maximum_radius: 0.3,
            head_radius: 0.1,
            head_box_center: [0., 0.05, 0.],
            head_box_half: [0.05; 3],
            feet_center: [0., -0.679, 0.],
            feet_radius: 0.2,
            friction: 0.,
            mass_kg: 75.,
            standing_membership: 1,
            standing_filter: 4_039_039_487,
            seated_membership: 1 << 10,
            seated_filter: 4_163_111_112,
            evidence: ManagedEvidence {
                assembly_sha256: String::new(),
                methods: Vec::new(),
            },
        }
    }

    #[test]
    fn capsule_reaches_head_height_and_widens_with_duck() {
        let r = reference();
        let s = Shapes::new(&r, Vector::new(0.1, 0.65, 0.2), 1.);
        assert!((s.radius - 0.3).abs() < 1e-6);
        assert!((s.body_center - Vector::new(0., -0.0175, 0.)).length() < 1e-5);
        assert!((s.body_half - (0.6675 - 0.3)).abs() < 1e-5);
        // Below the diameter the capsule is a sphere.
        let low = Shapes::new(&r, Vector::new(0., -0.6, 0.), 0.);
        assert_eq!(low.body_half, 0.);
        assert!((low.radius - 0.1).abs() < 1e-6);
    }

    #[test]
    fn colliders_wait_3_s_after_spawn_and_1_s_after_standing_up() {
        let r = reference();
        let mut physics = PhysicsWorld::empty_for_tests();
        let shapes = Shapes::new(&r, Vector::new(0., 0.65, 0.), 1.);
        let mut body = RemoteBody::new(&r, shapes, Pose::IDENTITY, 10., &mut physics);
        let player = player_at(&mut physics, 5.);
        body.tick(12.9, &mut physics, player);
        assert!(!body.enabled());
        body.tick(13., &mut physics, player);
        assert!(body.enabled());
        let capsule = body.colliders[0];
        let groups =
            |physics: &PhysicsWorld| physics.rapier().colliders[capsule].collision_groups();
        body.update(&r, shapes, true, 14., &mut physics);
        assert!(body.enabled(), "sitting down keeps the colliders");
        let solver = physics.rapier().colliders[capsule].solver_groups();
        assert_eq!(
            solver,
            InteractionGroups::none(),
            "a seated figure does not push its car"
        );
        assert!(physics.rapier().bodies[body.handle].is_kinematic());
        assert_eq!(groups(&physics).memberships.bits(), 1 << 10);
        body.update(&r, shapes, false, 20., &mut physics);
        assert!(!body.enabled());
        assert!(physics.rapier().bodies[body.handle].is_dynamic());
        assert_eq!(groups(&physics).memberships.bits(), 1);
        body.tick(20.9, &mut physics, player);
        assert!(!body.enabled());
        body.tick(21., &mut physics, player);
        assert!(body.enabled());
    }

    /// A kinematic player capsule at `x`.
    fn player_at(physics: &mut PhysicsWorld, x: f32) -> RigidBodyHandle {
        let handle = physics.insert_body(
            RigidBodyBuilder::kinematic_position_based().translation(Vector::new(x, 0., 0.)),
        );
        physics.insert_collider_with_parent(ColliderBuilder::capsule_y(0.5, 0.3).build(), handle);
        handle
    }

    #[test]
    fn colliders_wait_while_the_figure_overlaps_the_player() {
        let r = reference();
        let mut physics = PhysicsWorld::empty_for_tests();
        let shapes = Shapes::new(&r, Vector::new(0., 0.65, 0.), 1.);
        let mut body = RemoteBody::new(&r, shapes, Pose::IDENTITY, 0., &mut physics);
        let player = player_at(&mut physics, 0.2);
        body.tick(10., &mut physics, player);
        assert!(!body.enabled(), "a figure on the player stays open");
        body.place(
            &mut physics,
            Pose::from_translation(Vector::new(-3., 0., 0.)),
        );
        body.tick(10.1, &mut physics, player);
        assert!(body.enabled(), "a clear figure turns solid");
    }
}
