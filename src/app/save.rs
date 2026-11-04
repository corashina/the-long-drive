use super::*;
use simulation::boundary::{Change, SaveCommand};
use simulation::save_requests::{SaveAction, SaveKind, SaveRequest};

/// The save request that waits for the simulation.
enum Pending {
    /// The simulation runs a new game request.
    Waiting { id: u32 },
    /// A new game in the current runtime waits for the commit.
    Fresh(u32),
}
#[derive(Resource, Default)]
pub struct SaveRequests {
    pending: Option<Pending>,
    /// Save input of the next command.
    commands: Vec<SaveCommand>,
}

impl SaveRequests {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    /// Save requests of the next command.
    pub(crate) fn take_commands(&mut self) -> Vec<SaveCommand> {
        std::mem::take(&mut self.commands)
    }
}

/// Runs after the frame fill. It applies the save results and reads the next request.
pub fn dispatch(world: &mut World) {
    world.resource_scope(|world, mut requests: Mut<SaveRequests>| {
        results(world, &mut requests);
        if !game_has_save_request() {
            return;
        }
        let request = match SaveRequest::parse(&game_save_request()) {
            Ok(request) => request,
            Err(invalid) => {
                game_save_result(invalid.id, invalid.kind, "", &invalid.message);
                return;
            }
        };
        let kind = request.kind().name();
        let id = request.id;
        if let Err(error) = request_input(world, &mut requests, request) {
            game_save_result(id, kind, "", &error.to_string());
        }
    });
}

/// Answers Discard. The simulation runs the rest.
fn request_input(
    world: &mut World,
    requests: &mut SaveRequests,
    request: SaveRequest,
) -> Result<(), crate::save::SaveError> {
    let invalid = |message: &str| crate::save::SaveError::ReconstructionFailure(message.into());
    match request.action {
        SaveAction::Discard(prepared) => {
            if let Some(Pending::Fresh(id) | Pending::Waiting { id }) = requests.pending
                && id == prepared
            {
                requests.commands.push(SaveCommand::Cancel(id));
                requests.pending = None;
            }
            game_save_result(request.id, SaveKind::Discard.name(), "", "");
            return Ok(());
        }
        SaveAction::PrepareNewGame(_) => {
            if requests.pending.is_some() {
                return Err(invalid("A prepared session is pending"));
            }
            requests.pending = Some(Pending::Waiting { id: request.id });
        }
        SaveAction::Capture(_) | SaveAction::Commit(_) => {}
    }
    let reference = &world.resource::<RuntimeReference>().0;
    requests.commands.push(SaveCommand::Request {
        request,
        mode: super::game_mode(reference),
        epoch: world
            .resource::<crate::presentation::view_frame::SourceSceneEpoch>()
            .0,
    });
    Ok(())
}

/// Sends the simulation answers to the browser and follows the prepared session.
fn results(world: &mut World, requests: &mut SaveRequests) {
    for change in &world.resource::<SimFrameResource>().frame.changes {
        if let Change::SaveResult {
            id,
            kind,
            payload,
            error,
        } = change
        {
            game_save_result(*id, kind.name(), payload, error);
            match requests.pending {
                Some(Pending::Waiting { id: waiting }) if waiting == *id => {
                    requests.pending = error.is_empty().then_some(Pending::Fresh(*id));
                }
                Some(Pending::Fresh(_)) if *kind == SaveKind::Commit && error.is_empty() => {
                    requests.pending = None;
                }
                _ => {}
            }
        }
    }
}
