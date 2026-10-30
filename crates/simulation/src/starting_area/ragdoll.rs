//! Source `Death2` ragdoll spawn and the dead player follow.
use super::GarageRuntime;
use crate::vehicle::to_rapier;
use rapier3d::math::Pose;

impl GarageRuntime {
    /// `SpawnRagdol` at the live body pose, then the player colliders off.
    /// Wearables have no runtime yet (X01d), so `TakeOff` has nothing to do.
    pub(super) fn spawn_ragdoll(&mut self) {
        let area = &mut self.area;
        // `GetComponentInParent<Rigidbody>()`: the seat body, else the player body.
        let parent = area.player.seat_body().unwrap_or(area.player.body_handle());
        let velocity = self.physics.rapier().bodies[parent].linvel();
        let player = to_rapier(area.player.body_pose(&self.physics));
        let body = &area.body;
        area.ragdoll.spawn(
            &mut self.physics,
            player,
            |node| {
                let f = body.node_local(node)?;
                Some(player * Pose::from_parts(f.position, f.rotation))
            },
            velocity,
        );
        area.player.die(&mut self.physics);
    }
    /// Dead `Update`: Th on the ragdoll head, then `DeathLook` turns the head to the camera.
    pub(super) fn follow_ragdoll(&mut self) {
        let area = &mut self.area;
        let Some(head) = area.ragdoll.head_pose(&self.physics) else {
            return;
        };
        let th = to_rapier(area.player.view(&self.physics).head_pose).translation;
        area.player.follow(head.translation - th, &mut self.physics);
        let camera = to_rapier(area.player.view(&self.physics).camera_pose).rotation;
        area.ragdoll.set_head_rotation(&mut self.physics, camera);
    }
}
