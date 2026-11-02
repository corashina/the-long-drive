use super::*;
use serde::Deserialize;
use serde_json::value::RawValue;
use simulation::boundary::{Change, SaveCommand};

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
#[derive(Deserialize)]
struct Request {
    id: u32,
    kind: String,
    #[serde(default)]
    payload: Option<Box<RawValue>>,
}
impl Request {
    /// The prepared request that a Commit or Discard names.
    fn prepared(&self) -> Option<u32> {
        let payload = self.payload.as_ref()?;
        serde_json::from_str(payload.get()).ok()
    }
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

/// Runs after the frame fill: applies the save results and reads the next request.
pub fn dispatch(world: &mut World) {
    world.resource_scope(|world, mut requests: Mut<SaveRequests>| {
        results(world, &mut requests);
        if !game_has_save_request() {
            return;
        }
        let text = game_save_request();
        let request: Request = match serde_json::from_str(&text) {
            Ok(request) => request,
            Err(error) => {
                game_save_result(0, "Invalid", "", &error.to_string());
                return;
            }
        };
        if let Err(error) = request_input(world, &mut requests, &request, text) {
            game_save_result(request.id, &request.kind, "", &error.to_string());
        }
    });
}

/// Answers Discard. The simulation runs the rest.
fn request_input(
    world: &mut World,
    requests: &mut SaveRequests,
    request: &Request,
    json: String,
) -> Result<(), crate::save::SaveError> {
    let invalid = |message: &str| crate::save::SaveError::ReconstructionFailure(message.into());
    match request.kind.as_str() {
        "Discard" => {
            if let Some(Pending::Fresh(id) | Pending::Waiting { id }) = requests.pending
                && Some(id) == request.prepared()
            {
                requests.commands.push(SaveCommand::Cancel(id));
                requests.pending = None;
            }
            game_save_result(request.id, &request.kind, "", "");
            return Ok(());
        }
        "PrepareNewGame" => {
            if requests.pending.is_some() {
                return Err(invalid("A prepared session is pending"));
            }
            requests.pending = Some(Pending::Waiting { id: request.id });
        }
        _ => {}
    }
    let reference = &world.resource::<RuntimeReference>().0;
    requests.commands.push(SaveCommand::Request {
        json,
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
            game_save_result(*id, kind, payload, error);
            match requests.pending {
                Some(Pending::Waiting { id: waiting }) if waiting == *id => {
                    requests.pending = error.is_empty().then_some(Pending::Fresh(*id));
                }
                Some(Pending::Fresh(_)) if kind == "Commit" && error.is_empty() => {
                    requests.pending = None;
                }
                _ => {}
            }
        }
    }
}
