//! Source footsteps: `fpscontroller` step triggers and `stepSoundScript.Step`.
use super::{AudioAction, AudioKind, AudioState, SurvivalAudioEvent};
use crate::reference::{FootstepAudioReference, StepSurface};
use crate::starting_area::StartingArea;
use crate::vehicle::{PhysicsWorld, STEP_SURFACE_MASK, STEP_SURFACE_SHIFT, to_rapier};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray, Vector};

const ROAD: usize = 0;
const DESERT: usize = 1;
const WINTER: usize = 2;
const GRASS: usize = 3;
const WOOD: usize = 4;

/// Surface index in source order. `biome` runs only for sand.
pub fn surface(
    code: u8,
    membership: u32,
    sand_layers: &[u32],
    biome: impl FnOnce() -> u8,
) -> usize {
    // otherStepSoundScript, then sand layer or tag, then tag Wood, then road.
    // Codes 1 to 5 are the surfaces in source order.
    if (1..=5).contains(&code) {
        return code as usize - 1;
    }
    if code == StepSurface::Sand as u8 || sand_layers.iter().any(|l| membership & (1 << l) != 0) {
        return match biome() {
            1 => WINTER,
            2 => GRASS,
            _ => DESERT,
        };
    }
    if code == StepSurface::WoodTag as u8 {
        WOOD
    } else {
        ROAD
    }
}

/// Source `Step` modifier: duck value, plus 0.15 while run is held.
pub fn gain(volume: f32, posture: f32, run: bool, step_gain: f32) -> f32 {
    volume * (posture + if run { 0.15 } else { 0. }) * step_gain
}

/// Clip and gain of a step on a hit collider `(user_data, membership)`. `sample` is in 0..1.
fn step_sound(
    r: &FootstepAudioReference,
    hit: Option<(u128, u32)>,
    posture: f32,
    run: bool,
    sample: f32,
    biome: impl FnOnce() -> u8,
) -> Option<(u32, f32)> {
    let (data, membership) = hit?;
    let code = ((data & STEP_SURFACE_MASK) >> STEP_SURFACE_SHIFT) as u8;
    let s = &r.surfaces[surface(code, membership, &r.sand_layers, biome)];
    let index = ((sample * s.clips.len() as f32) as usize).min(s.clips.len() - 1);
    Some((s.clips[index], gain(s.volume, posture, run, r.step_gain)))
}

#[derive(Default)]
pub struct FootstepState {
    last_grounded: bool,
}

impl FootstepState {
    /// True when the source plays a step: a walk bob beat or a grounded change, not seated.
    fn step(&mut self, seated: bool, grounded: bool, beat: bool) -> bool {
        let changed = grounded != self.last_grounded;
        self.last_grounded = grounded;
        !seated && (beat || changed)
    }
}

impl AudioState {
    /// Run after the walk bob step. `biome` takes an absolute position.
    pub fn footsteps(
        &mut self,
        area: &StartingArea,
        physics: &PhysicsWorld,
        run: bool,
        biome: impl FnOnce([f64; 3]) -> u8,
    ) {
        let player = &area.player;
        let seated = player.seated();
        if !self
            .footsteps
            .step(seated, player.grounded(), player.footstep_beat())
        {
            return;
        }
        let r = &self.reference.footsteps;
        let body = to_rapier(player.body_pose(physics));
        let origin = body.transform_point(Vector::from_array(r.local_position));
        let own = player.body_handle();
        let mask = r.ray_mask;
        let floor = |_: ColliderHandle, c: &Collider| {
            !c.is_sensor()
                && c.parent() != Some(own)
                && c.collision_groups().memberships.bits() & mask != 0
        };
        let hit = physics
            .queries(QueryFilter::default().predicate(&floor))
            .cast_ray(
                &Ray::new(origin, body.rotation * -Vector::Y),
                r.ray_distance,
                true,
            )
            .map(|(handle, _)| {
                let c = &physics.rapier().colliders[handle];
                (c.user_data, c.collision_groups().memberships.bits())
            });
        let position = std::array::from_fn(|i| area.origin[i] + f64::from(origin[i]));
        let sample = self.sample();
        let Some((clip, gain)) = step_sound(
            &self.reference.footsteps,
            hit,
            player.posture(),
            run,
            sample,
            || biome(position),
        ) else {
            return;
        };
        let [low, high] = self.reference.shared.random_pitch;
        let pitch = low + (high - low) * self.sample();
        self.survival_event(SurvivalAudioEvent {
            session: self.session,
            emitter: 0,
            kind: AudioKind::Footstep,
            action: AudioAction::Start,
            clip,
            gain,
            pitch,
            position,
        });
    }
}
