use super::Drive;
use crate::browser::{game_performance_budget, game_performance_counters, game_performance_frame};
use crate::presentation::sim_frame::SimFrameResource;
use bevy::prelude::*;
use simulation::world::metrics::BUDGET_LABELS;

pub(super) fn publish(
    drive: Res<Drive>,
    sim: Res<SimFrameResource>,
    archetypes: &bevy::ecs::archetype::Archetypes,
) {
    let perf = &sim.frame.state.perf;
    let (update_ms, render_ms) = super::frame_cpu::last_ms();
    if !game_performance_frame(
        perf.physics_ms,
        perf.rapier_ms,
        drive.world_ms,
        update_ms,
        render_ms,
    ) {
        return;
    }
    for (index, (budget, (name, unit))) in sim.frame.budgets.iter().zip(BUDGET_LABELS).enumerate() {
        game_performance_budget(index as u32, name, budget.used, budget.limit, unit);
    }
    game_performance_counters(
        perf.frame_steps,
        perf.contacts,
        perf.bodies,
        perf.colliders,
        perf.storms,
        perf.storm_strength,
        perf.weather_particles,
        perf.weather_kind,
        perf.weather_cells,
        perf.weather_rain,
        perf.weather_wetness,
        archetypes.iter().map(|a| a.len()).sum(),
        perf.dynamic_bodies,
        perf.awake_bodies,
        perf.ccd_bodies,
        perf.contact_pairs,
    );
}
