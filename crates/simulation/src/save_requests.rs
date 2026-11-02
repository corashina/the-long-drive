//! Browser save requests in the simulation: capture, new game and commit.
use crate::boundary::{Change, SaveCommand, SimFrame};
use crate::driver::SimDriver;
use crate::reference::ReferenceManifest;
use crate::save::fresh_permit::FreshSessionPermit;
use crate::save::{SaveError, SaveSnapshot, SourceStamp};
use crate::world::GameMode;
use serde::{Deserialize, Serialize};

pub(crate) struct SaveState {
    source: SourceStamp,
    /// The new game in the current runtime.
    prepared: Option<FreshSessionPermit>,
    /// Results that the next frame carries.
    results: Vec<Change>,
}

#[derive(Deserialize)]
struct Request {
    id: u32,
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
}
#[derive(Serialize)]
struct CaptureResult<'a> {
    save: &'a SaveSnapshot,
    summary: CaptureSummary,
}
#[derive(Serialize)]
struct CaptureSummary {
    seed: i32,
    driven_km: f64,
    build_id: u32,
}

impl SaveState {
    pub(crate) fn new(source: SourceStamp) -> Self {
        Self {
            source,
            prepared: None,
            results: Vec::new(),
        }
    }
}

impl SimDriver {
    /// Applies the save commands of a frame, after its input.
    pub(crate) fn accept_saves(&mut self, reference: &ReferenceManifest, saves: Vec<SaveCommand>) {
        for save in saves {
            match save {
                SaveCommand::Request { json, mode, epoch } => {
                    let (id, kind, result) = match serde_json::from_str::<Request>(&json) {
                        Ok(request) => {
                            let result = self.request(reference, &request, mode, epoch);
                            (request.id, request.kind, result)
                        }
                        Err(error) => (
                            0,
                            "Invalid".into(),
                            Err(SaveError::CorruptData(error.to_string())),
                        ),
                    };
                    let (payload, error) = match result {
                        Ok(None) => continue,
                        Ok(Some(payload)) => (payload, String::new()),
                        Err(error) => (String::new(), error.to_string()),
                    };
                    self.save.results.push(Change::SaveResult {
                        id,
                        kind,
                        payload,
                        error,
                    });
                }
                SaveCommand::Cancel(id) => {
                    if self.save.prepared.is_some_and(|p| p.request_id == id) {
                        self.save.prepared = None;
                    }
                }
            }
        }
    }

    /// Runs one request. `None`: no answer.
    fn request(
        &mut self,
        reference: &ReferenceManifest,
        request: &Request,
        mode: GameMode,
        epoch: u32,
    ) -> Result<Option<String>, SaveError> {
        let invalid = |message: &str| SaveError::ReconstructionFailure(message.into());
        match request.kind.as_str() {
            "Commit" => {
                #[cfg(target_arch = "wasm32")]
                let _profile =
                    crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::SaveCommit);
                let id = request
                    .payload
                    .as_u64()
                    .and_then(|id| u32::try_from(id).ok());
                let permit = self
                    .save
                    .prepared
                    .take()
                    .ok_or_else(|| invalid("Prepared session is missing"))?;
                let id = id.ok_or_else(|| invalid("Prepared session is stale"));
                let result = id.and_then(|id| {
                    if !self.ready {
                        return Err(invalid("The garage is not ready"));
                    }
                    permit.commit(id, &mut self.runtime, epoch)
                });
                if let Err(error) = result {
                    self.save.prepared = Some(permit);
                    return Err(error);
                }
                self.clear_input();
                self.frame_steps = 0;
                Ok(Some(String::new()))
            }
            "Capture" | "PrepareNewGame" => {
                #[cfg(target_arch = "wasm32")]
                let _profile =
                    crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::SavePrepare);
                if !self.ready {
                    return Err(invalid("The garage is not ready"));
                }
                let payload = &request.payload;
                let autosave = request.kind == "Capture"
                    && payload
                        .get("autosave")
                        .and_then(serde_json::Value::as_bool)
                        .ok_or_else(|| invalid("Missing autosave flag"))?;
                if !autosave && self.runtime.started && !self.runtime.paused {
                    return Err(invalid("Pause the game first"));
                }
                // Source guests have no save: the host world is the one world.
                if request.kind == "Capture" && self.net.role() == crate::net::NetRole::Guest {
                    return Err(invalid("Guests cannot save"));
                }
                if request.kind == "Capture" {
                    if !autosave {
                        self.clear_input();
                    }
                    let snapshot = self.runtime.capture_save(
                        reference,
                        &self.save.source,
                        payload
                            .get("date")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| invalid("Missing capture date"))?,
                    )?;
                    return serde_json::to_string(&CaptureResult {
                        save: &snapshot,
                        summary: CaptureSummary {
                            seed: snapshot.world.seed,
                            driven_km: snapshot.progress.driven_km,
                            build_id: snapshot.source.build_id,
                        },
                    })
                    .map(Some)
                    .map_err(|error| SaveError::CorruptData(error.to_string()));
                }
                if self.save.prepared.is_some() {
                    return Err(invalid("A prepared session is pending"));
                }
                let draw_distance = payload
                    .get("draw_distance")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| invalid("Missing draw distance"))?
                    as f32;
                let runtime = &self.runtime;
                // A new game starts the garage world. The browser reloads the page for a
                // started game or another game mode, so the garage has the new mode.
                if runtime.started
                    || runtime.stream.mode() != mode
                    || !runtime
                        .distances
                        .matches(&reference.world.draw_distance, draw_distance)
                {
                    return Err(invalid("New game needs a garage in its game mode"));
                }
                self.save.prepared = Some(FreshSessionPermit::prepare(request.id, runtime, epoch)?);
                Ok(Some(request.id.to_string()))
            }
            _ => Err(invalid("Unknown save request")),
        }
    }

    /// Writes the save results into `frame`.
    pub(crate) fn fill_saves(&mut self, frame: &mut SimFrame) {
        frame.changes.append(&mut self.save.results);
    }
}
