use super::*;
use crate::vehicle::PhysicsWorld;
use rapier3d::math::Vector;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

fn empty_world() -> PhysicsWorld {
    PhysicsWorld::empty_for_tests()
}

#[test]
fn world_sync_explicit_moves_survive_query_refresh_for_all_body_types() {
    for builder in [
        RigidBodyBuilder::fixed(),
        RigidBodyBuilder::dynamic().sleeping(true),
        RigidBodyBuilder::kinematic_position_based(),
        RigidBodyBuilder::kinematic_velocity_based(),
    ] {
        let mut world = empty_world();
        let body = world.insert_body(builder);
        assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
        world.clear_presentation_bodies();
        world.edit_body(body, |body| body.set_translation(Vector::X * 7.0, false));
        world.refresh_queries();
        assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
        assert_eq!(world.rapier().bodies[body].translation(), Vector::X * 7.0);
        assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
        world.try_edit_body(body, |body| body.set_translation(Vector::Y * 3.0, false));
        assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
        world.clear_presentation_bodies();
        assert_eq!(world.presentation_bodies().count(), 0);
    }
}

#[test]
fn world_sync_every_step_keeps_pre_step_sleepers_and_post_step_wakes() {
    let mut world = empty_world();
    let (body, _) = world.insert(RigidBodyBuilder::dynamic(), ColliderBuilder::ball(0.5));
    world.step_physics();
    assert!(
        world
            .physics
            .islands
            .active_bodies()
            .any(|handle| handle == body)
    );
    world.clear_presentation_bodies();
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);

    let mut slept = false;
    for _ in 0..600 {
        let was_active = world
            .physics
            .islands
            .active_bodies()
            .any(|handle| handle == body);
        world.clear_presentation_bodies();
        world.step_physics();
        let is_active = world
            .physics
            .islands
            .active_bodies()
            .any(|handle| handle == body);
        assert_eq!(
            world.presentation_bodies().count(),
            usize::from(was_active || is_active)
        );
        if was_active && !is_active {
            assert!(world.rapier().bodies[body].is_sleeping());
            assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
            slept = true;
            break;
        }
    }
    assert!(slept, "The body must enter sleep during this test");
    world.step_physics();
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
    world.clear_presentation_bodies();
    world.step_physics();
    assert_eq!(world.presentation_bodies().count(), 0);

    world.edit_body(body, |body| body.set_linvel(Vector::X, true));
    world.clear_presentation_bodies();
    assert!(
        !world
            .physics
            .islands
            .active_bodies()
            .any(|handle| handle == body)
    );
    world.step_physics();
    assert!(
        world
            .physics
            .islands
            .active_bodies()
            .any(|handle| handle == body)
    );
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
    assert!(world.rapier().bodies[body].translation().x > 0.0);
}

#[test]
fn world_sync_removal_and_reused_generations_remain_distinct() {
    let mut world = empty_world();
    let retired = world.insert_body(RigidBodyBuilder::fixed());
    world.clear_presentation_bodies();
    world.remove_body(retired).unwrap();
    let replacement = world.insert_body(RigidBodyBuilder::fixed());
    assert_eq!(retired.into_raw_parts().0, replacement.into_raw_parts().0);
    assert_ne!(retired, replacement);
    world.presentation_bodies.mark(retired);
    world.edit_body(replacement, |_| ());
    assert_eq!(
        world.presentation_bodies().collect::<Vec<_>>(),
        [retired, replacement]
    );
    assert!(world.try_edit_body(retired, |_| ()).is_none());
    world.refresh_queries();
    assert_eq!(
        world.presentation_bodies().collect::<Vec<_>>(),
        [retired, replacement]
    );
    world.clear_presentation_bodies();
    world.edit_body(replacement, |body| body.set_translation(Vector::X, false));
    assert_eq!(
        world.presentation_bodies().collect::<Vec<_>>(),
        [replacement]
    );
}

#[test]
fn world_sync_dedup_and_clear_reuse_queue_capacity() {
    let mut queue = PresentationBodies::default();
    for slot in 0..128 {
        queue.mark(RigidBodyHandle::from_raw_parts(slot, 0));
    }
    let capacities = (
        queue.handles.capacity(),
        queue.queued.capacity(),
        queue.retained_bytes(),
    );
    for _ in 0..16 {
        for slot in 0..128 {
            queue.mark(RigidBodyHandle::from_raw_parts(slot, 0));
            queue.mark(RigidBodyHandle::from_raw_parts(slot, 0));
        }
        assert_eq!(queue.handles.len(), 128);
        assert_eq!(
            capacities,
            (
                queue.handles.capacity(),
                queue.queued.capacity(),
                queue.retained_bytes()
            )
        );
        queue.clear();
        assert!(queue.handles.is_empty());
        assert!(queue.queued.is_empty());
        assert_eq!(
            capacities,
            (
                queue.handles.capacity(),
                queue.queued.capacity(),
                queue.retained_bytes()
            )
        );
    }
}

#[test]
fn world_sync_unchanged_inactive_edits_do_not_queue_bodies() {
    let mut world = empty_world();
    for _ in 0..512 {
        world.insert_body(RigidBodyBuilder::fixed());
    }
    let sleeper = world.insert_body(RigidBodyBuilder::dynamic().sleeping(true));
    world.clear_presentation_bodies();
    world.edit_bodies(|_, body, _| body.set_position(*body.position(), false));
    world.edit_body(sleeper, |body| body.set_enabled(body.is_enabled()));
    world.try_edit_body(sleeper, |body| {
        body.set_next_kinematic_position(*body.next_position())
    });
    world.refresh_queries();
    assert_eq!(world.presentation_bodies().count(), 0);
    world.edit_body(sleeper, |body| body.set_translation(Vector::Z, false));
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [sleeper]);
}

#[test]
fn world_sync_enabled_and_kinematic_target_changes_queue_once() {
    let mut world = empty_world();
    let body = world.insert_body(RigidBodyBuilder::kinematic_position_based());
    world.clear_presentation_bodies();
    world.edit_body(body, |body| body.set_enabled(false));
    world.edit_body(body, |body| body.set_enabled(false));
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
    world.clear_presentation_bodies();
    world.edit_body(body, |body| {
        body.set_next_kinematic_position(rapier3d::math::Pose::translation(4.0, 0.0, 0.0));
    });
    assert_eq!(world.presentation_bodies().collect::<Vec<_>>(), [body]);
}
