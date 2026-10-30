use super::*;
use crate::reference::{EndpointReactionReference, ItemId, NodeId};
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::vehicle::VehiclePose;
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{RigidBody, RigidBodyHandle};

#[track_caller]
fn near(actual: Vector, expected: Vector) {
    assert!(
        (actual - expected).length() < 0.02,
        "{actual:?} != {expected:?}"
    );
}

fn momentum(body: &RigidBody) -> (Vector, Vector) {
    (
        body.linvel() * body.mass(),
        body.mass_properties().effective_angular_inertia() * body.angvel(),
    )
}

fn nonzero_inertia(body: &RigidBody) {
    assert!(body.is_dynamic() && body.is_enabled() && body.mass() > 0.);
    for axis in [Vector::X, Vector::Y, Vector::Z] {
        assert!((body.mass_properties().effective_angular_inertia() * axis).length() > 0.);
        assert!((body.mass_properties().effective_world_inv_inertia * axis).length() > 0.);
    }
}

fn zero(run: &mut GarageRuntime, body: RigidBodyHandle) {
    run.physics.edit_body(body, |b| {
        b.set_linvel(Vector::ZERO, true);
        b.set_angvel(Vector::ZERO, true);
    });
}

fn update(run: &mut GarageRuntime, item: ItemId, input: [f32; 2], dt: f32) {
    let m = &mut run.area.interaction.mechanisms;
    m.step(
        item,
        FrameInput {
            held: keys::USE,
            mouse: [input[0], -input[1]],
            ..FrameInput::default()
        },
        VehiclePose {
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
        dt,
    );
    m.sync(&mut run.physics);
}

fn assert_impulse(
    run: &mut GarageRuntime,
    node: NodeId,
    body: RigidBodyHandle,
    axes: Vector,
    force: f32,
) {
    let pose = run.area.interaction.mechanisms.node_pose(node).unwrap();
    let impulse = Rotation::from_array(pose.rotation) * axes * (force * FIXED_DT);
    let torque = (Vector::from_array(pose.position)
        - run.physics.rapier().bodies[body].center_of_mass())
    .cross(impulse);
    let properties = run.physics.rapier().bodies[body].mass_properties();
    let expected_spin = properties.effective_world_inv_inertia * torque;
    let expected_angular = properties.effective_angular_inertia() * expected_spin;
    run.area
        .interaction
        .mechanisms
        .apply_reactions(&mut run.physics);
    let (linear, angular) = momentum(&run.physics.rapier().bodies[body]);
    near(run.physics.rapier().bodies[body].angvel(), expected_spin);
    near(run.physics.rapier().bodies[body].user_force(), Vector::ZERO);
    near(
        run.physics.rapier().bodies[body].user_torque(),
        Vector::ZERO,
    );
    near(linear, impulse);
    near(angular, expected_angular);
    run.area
        .interaction
        .mechanisms
        .apply_reactions(&mut run.physics);
    let (again, spin) = momentum(&run.physics.rapier().bodies[body]);
    near(again, linear);
    near(spin, angular);
}

#[test]
#[ignore = "requires the installed source export"]
fn g10_generated_endpoint_root() {
    use crate::reference::DragMode;
    use crate::world::{StructureRecipe, WorldOrigin};
    let (reference, mut run) = load_garage();
    let row = reference.world.mechanisms.iter().find(|row| {
        matches!(row.recipe.drag.mode, DragMode::Direct)
            && matches!(&row.recipe.reaction, EndpointReactionReference::Rotation { force, events, .. }
                if *force > 0. && *force != 10000. && (events[0] || events[1]))
            && row.recipe.rotation.as_ref().is_some_and(|r| r.limits_degrees[0][0] == r.limits_degrees[0][1]
                && r.limits_degrees[1][0] != r.limits_degrees[1][1])
            && reference.world.prototypes.iter().find(|p| p.source == row.prototype).is_some_and(|p| {
                p.bodies.len() == 1 && p.bodies[0].node == 0 && !p.bodies[0].kinematic && p.bodies[0].constraints == 0
            })
    }).expect("generated source reaction with its own root body");
    let source_prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == row.prototype)
        .unwrap();
    let EndpointReactionReference::Rotation {
        node,
        axes,
        inverted,
        force,
        events,
    } = &row.recipe.reaction
    else {
        unreachable!()
    };
    let rotation = row.recipe.rotation.as_ref().unwrap();
    let rules = crate::world::spawn_rules::SourceRules::new(&reference.world);
    for present in [true, false] {
        let mut prototype = source_prototype.clone();
        let mut placement = super::super::replay_placement(
            &prototype.source,
            if present { 9821 } else { 9822 },
            [50.; 3],
        );
        placement.scale = prototype.prefab_pose.scale;
        let recipe = StructureRecipe::new(&reference.world, &prototype).unwrap();
        let mut instance = recipe
            .publish(
                placement.clone(),
                WorldOrigin::default(),
                false,
                &mut run.physics,
                &mut crate::world::TriggerRoles::default(),
            )
            .unwrap();
        let root_body = instance.bodies[0];
        run.physics.step_physics();
        let body = &run.physics.rapier().bodies[root_body];
        nonzero_inertia(body);
        assert!(run.physics.body_active(root_body));
        assert_eq!(body.mass(), prototype.bodies[0].mass_kg);
        let mut content = crate::world::contents::ContentInstance::for_structure(placement.clone());
        let root_item = run
            .area
            .items
            .insert_generated(placement.id, &prototype, placement.pose)
            .unwrap();
        assert!(run.area.items.get(root_item).unwrap().body.is_none());
        content.item = Some(root_item);
        if !present {
            // A live item body must not substitute for an absent source root body.
            prototype.bodies.clear();
            run.area.items.set_body(root_item, Some(root_body));
        }
        crate::world::content_mechanisms::bind(
            &mut content,
            &mut instance,
            &prototype,
            &reference,
            &rules,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
        )
        .unwrap();
        let item = if row.node == 0 {
            root_item
        } else {
            content
                .bindings
                .iter()
                .find(|b| b.source == row.recipe.source)
                .unwrap()
                .item
        };
        if present {
            assert_ne!(
                run.area.items.get(root_item).unwrap().body,
                Some(root_body),
                "binding must preserve existing item ownership"
            );
        }
        let control_node = content.nodes[node.0 as usize - 1];
        let mut saved = run.area.interaction.mechanisms.snapshot(item).unwrap();
        saved.degrees[1] = (rotation.limits_degrees[1][0] + rotation.limits_degrees[1][1]) * 0.5;
        saved.enabled = true;
        run.area
            .interaction
            .mechanisms
            .restore(item, saved)
            .unwrap();
        run.area.interaction.mechanisms.sync(&mut run.physics);
        zero(&mut run, root_body);
        let input =
            10000. * if events[0] { -1. } else { 1. } * if rotation.inverted[1] { -1. } else { 1. };
        update(&mut run, item, [0., input], 0.02);
        let local_axes = Vector::new(
            axes[0] as u8 as f32,
            axes[1] as u8 as f32,
            -(axes[2] as u8 as f32),
        ) * if *inverted { -1. } else { 1. };
        assert_eq!(
            run.area
                .interaction
                .mechanisms
                .rotation_degrees(item)
                .unwrap()[1],
            rotation.limits_degrees[1][if events[0] { 0 } else { 1 }]
        );
        assert_impulse(
            &mut run,
            control_node,
            root_body,
            local_axes,
            if present { *force } else { 0. },
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn g10_world_library_identity() {
    let reference = super::super::reference();
    let mut manifest_only = reference.clone();
    manifest_only.library.manifest_sha256 = "e".repeat(64);
    assert!(manifest_only.world.validate(&manifest_only).is_err());
    let mut runtime_only = reference.clone();
    std::sync::Arc::make_mut(&mut runtime_only.world)
        .library
        .manifest_sha256 = "e".repeat(64);
    assert!(
        runtime_only
            .world
            .validate(&runtime_only)
            .unwrap_err()
            .to_string()
            .contains("world library receipt")
    );
}
