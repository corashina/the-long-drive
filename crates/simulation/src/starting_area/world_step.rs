//! World logic of one frame step. The renderer prepares after it.
use super::GarageRuntime;
use crate::{
    boundary::{LampAdmission, WorldCommand},
    reference::{ReferenceError, ReferenceManifest},
    world::{GeneratedId, PresentationGrant, RendererFacts, RequiredView, stream_profile},
};

/// Renderer input that the next world step uses.
#[derive(Default)]
pub struct WorldInput {
    /// Merged commands since the last step.
    pending: WorldCommand,
    views: Vec<RequiredView>,
    lamps: Option<Vec<GeneratedId>>,
}
impl WorldInput {
    pub fn accept(&mut self, mut command: WorldCommand) {
        self.views = std::mem::take(&mut command.views);
        match std::mem::take(&mut command.lamps) {
            LampAdmission::Same => {}
            LampAdmission::Pending => self.lamps = None,
            LampAdmission::Ready(lamps) => self.lamps = Some(lamps),
        }
        self.pending.merge(command);
    }
    pub fn take_failure(&mut self) -> Option<String> {
        self.pending.failure.take()
    }
}

impl GarageRuntime {
    /// One world step from the renderer input. The grant answers the renderer report.
    pub fn step_world(
        &mut self,
        reference: &ReferenceManifest,
        input: &mut WorldInput,
        budget_ms: f64,
    ) -> Result<Option<PresentationGrant>, ReferenceError> {
        let mut command = std::mem::take(&mut input.pending);
        self.stream.apply_renderer_facts(&mut command.facts)?;
        // Own work runs on the given-back credit first. The renderer gets the room it leaves.
        if let Some(report) = command.report {
            self.stream.take_presentation_report(report);
        }
        self.advance_world(
            reference,
            &input.views,
            &mut command.facts,
            input.lamps.as_deref(),
            budget_ms,
        )?;
        Ok(command.report.map(|_| self.stream.grant_presentation()))
    }
    /// Applies the renderer facts, publishes the renderer-ready work, then prepares the next work.
    /// `admitted_lights` are the lamps the renderer admitted in its last step.
    pub fn advance_world(
        &mut self,
        reference: &ReferenceManifest,
        required_views: &[RequiredView],
        facts: &mut RendererFacts,
        admitted_lights: Option<&[GeneratedId]>,
        budget_ms: f64,
    ) -> Result<(), ReferenceError> {
        let mut timer = stream_profile::Timer::new(stream_profile::Stage::Publication);
        self.stream.apply_renderer_facts(facts)?;
        // Light admission and complete renderer preparation precede collision mutation.
        if let Some(required) = admitted_lights {
            self.stream.publish(&mut self.physics, required)?;
        }
        timer.next(stream_profile::Stage::Supplies);
        self.stream.sync_supplies(
            reference,
            &mut self.area.items,
            &mut self.area.interaction,
            &mut self.physics,
            &mut self.wheels,
            &mut self.vehicles,
            &mut self.powertrain,
        )?;
        timer.next(stream_profile::Stage::Capture);
        self.stream.capture_changes(
            reference,
            &mut self.area.items,
            &self.area.interaction,
            &self.physics,
        );
        timer.next(stream_profile::Stage::FinalCleanup);
        self.stream.retire_structures(
            &mut self.physics,
            &mut self.area.items,
            &mut self.area.interaction,
            &mut self.wheels,
            &mut self.vehicles,
            &mut self.powertrain,
        )?;
        timer.next(stream_profile::Stage::Stream);
        self.stream.prepare(budget_ms);
        self.stream.select_required(required_views);
        Ok(())
    }
}
