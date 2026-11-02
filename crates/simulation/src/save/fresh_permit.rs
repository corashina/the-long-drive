//! First-start authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshSessionPermit {
    pub request_id: u32,
    pub session: u64,
    pub generation: u32,
}
impl FreshSessionPermit {
    pub fn prepare(
        request_id: u32,
        runtime: &crate::starting_area::GarageRuntime,
        generation: u32,
    ) -> Result<Self, super::SaveError> {
        if runtime.started {
            return Err(super::SaveError::ReconstructionFailure(
                "The session has started".into(),
            ));
        }
        Ok(Self {
            request_id,
            session: runtime.stream.frame().request.session,
            generation,
        })
    }
    pub fn commit(
        self,
        request_id: u32,
        runtime: &mut crate::starting_area::GarageRuntime,
        generation: u32,
    ) -> Result<(), super::SaveError> {
        if runtime.started
            || !self.matches(
                request_id,
                runtime.stream.frame().request.session,
                generation,
            )
        {
            return Err(super::SaveError::ReconstructionFailure(
                "Prepared session is stale".into(),
            ));
        }
        runtime.clear_input();
        runtime.started = true;
        runtime.paused = true;
        Ok(())
    }

    pub fn matches(self, request_id: u32, session: u64, generation: u32) -> bool {
        self == Self {
            request_id,
            session,
            generation,
        }
    }
}
