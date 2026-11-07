//! Browser save requests: the typed request, its answer, and the simulation side.
//! The browser sends JSON `{id, kind, payload}`. The renderer parses it once with
//! `SaveRequest::parse` and answers Discard. The simulation runs the rest.
use crate::boundary::{Change, SaveCommand, SimFrame};
use crate::driver::SimDriver;
use crate::reference::ReferenceManifest;
use crate::save::fresh_permit::FreshSessionPermit;
use crate::save::{SaveError, SourceStamp};
use crate::world::GameMode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveKind {
    Capture,
    PrepareNewGame,
    Commit,
    Discard,
}

impl SaveKind {
    /// The browser name of the kind.
    pub fn name(self) -> &'static str {
        match self {
            Self::Capture => "Capture",
            Self::PrepareNewGame => "PrepareNewGame",
            Self::Commit => "Commit",
            Self::Discard => "Discard",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRequest {
    /// ISO date of the save.
    pub date: String,
    pub autosave: bool,
    /// The world for a joining guest. Its player starts in the garage.
    pub guest: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewGameRequest {
    pub draw_distance: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SaveAction {
    Capture(CaptureRequest),
    PrepareNewGame(NewGameRequest),
    /// Commits the prepared session of this request id.
    Commit(u32),
    /// Drops the prepared session of this request id.
    Discard(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveRequest {
    pub id: u32,
    pub action: SaveAction,
}

/// A browser request that does not parse. `id` is 0 when the id did not parse.
#[derive(Debug, PartialEq)]
pub struct InvalidRequest {
    pub id: u32,
    pub kind: &'static str,
    pub message: String,
}

impl SaveRequest {
    pub fn parse(json: &str) -> Result<Self, InvalidRequest> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            id: u32,
            kind: SaveKind,
            #[serde(default)]
            payload: serde_json::Value,
        }
        let wire: Wire = serde_json::from_str(json).map_err(|error| InvalidRequest {
            id: 0,
            kind: "Invalid",
            message: error.to_string(),
        })?;
        let payload = wire.payload;
        let action = match wire.kind {
            SaveKind::Capture => serde_json::from_value(payload).map(SaveAction::Capture),
            SaveKind::PrepareNewGame => {
                serde_json::from_value(payload).map(SaveAction::PrepareNewGame)
            }
            SaveKind::Commit => serde_json::from_value(payload).map(SaveAction::Commit),
            SaveKind::Discard => serde_json::from_value(payload).map(SaveAction::Discard),
        };
        action
            .map(|action| Self {
                id: wire.id,
                action,
            })
            .map_err(|error| InvalidRequest {
                id: wire.id,
                kind: wire.kind.name(),
                message: error.to_string(),
            })
    }
    pub fn kind(&self) -> SaveKind {
        match self.action {
            SaveAction::Capture(_) => SaveKind::Capture,
            SaveAction::PrepareNewGame(_) => SaveKind::PrepareNewGame,
            SaveAction::Commit(_) => SaveKind::Commit,
            SaveAction::Discard(_) => SaveKind::Discard,
        }
    }
}

pub(crate) struct SaveState {
    source: SourceStamp,
    /// The new game in the current runtime.
    prepared: Option<FreshSessionPermit>,
    /// Results that the next frame carries.
    results: Vec<Change>,
}

/// The save list fields of a capture. The save itself goes as JSON bytes beside it.
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
                SaveCommand::Request {
                    request,
                    mode,
                    epoch,
                } => {
                    let ((payload, save), error) =
                        match self.request(reference, &request, mode, epoch) {
                            Ok(None) => continue,
                            Ok(Some(answer)) => (answer, String::new()),
                            Err(error) => ((String::new(), Vec::new()), error.to_string()),
                        };
                    self.save.results.push(Change::SaveResult {
                        id: request.id,
                        kind: request.kind(),
                        payload,
                        save,
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
        request: &SaveRequest,
        mode: GameMode,
        epoch: u32,
    ) -> Result<Option<(String, Vec<u8>)>, SaveError> {
        let invalid = |message: &str| SaveError::ReconstructionFailure(message.into());
        match &request.action {
            SaveAction::Commit(id) => {
                #[cfg(target_arch = "wasm32")]
                let _profile =
                    crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::SaveCommit);
                let permit = self
                    .save
                    .prepared
                    .take()
                    .ok_or_else(|| invalid("Prepared session is missing"))?;
                let result = if self.ready {
                    permit.commit(*id, &mut self.runtime, epoch)
                } else {
                    Err(invalid("The garage is not ready"))
                };
                if let Err(error) = result {
                    self.save.prepared = Some(permit);
                    return Err(error);
                }
                self.clear_input();
                self.frame_steps = 0;
                Ok(Some((String::new(), Vec::new())))
            }
            SaveAction::Capture(capture) => {
                #[cfg(target_arch = "wasm32")]
                let _profile =
                    crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::SavePrepare);
                if !self.ready {
                    return Err(invalid("The garage is not ready"));
                }
                if !capture.autosave && self.runtime.started && !self.runtime.menu {
                    return Err(invalid("Pause the game first"));
                }
                // Source guests have no save. The host world is the only world.
                if self.net.role() == crate::net::NetRole::Guest {
                    return Err(invalid("Guests cannot save"));
                }
                if !capture.autosave {
                    self.clear_input();
                }
                let mut snapshot =
                    self.runtime
                        .capture_save(reference, &self.save.source, &capture.date)?;
                if capture.guest {
                    crate::starting_area::garage_start(reference, &mut snapshot.player);
                }
                let corrupt = |error: serde_json::Error| SaveError::CorruptData(error.to_string());
                let summary = serde_json::to_string(&CaptureSummary {
                    seed: snapshot.world.seed,
                    driven_km: snapshot.progress.driven_km,
                    build_id: snapshot.source.build_id,
                })
                .map_err(corrupt)?;
                let save = serde_json::to_vec(&snapshot).map_err(corrupt)?;
                Ok(Some((summary, save)))
            }
            SaveAction::PrepareNewGame(new_game) => {
                #[cfg(target_arch = "wasm32")]
                let _profile =
                    crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::SavePrepare);
                if !self.ready {
                    return Err(invalid("The garage is not ready"));
                }
                if self.runtime.started && !self.runtime.menu {
                    return Err(invalid("Pause the game first"));
                }
                if self.save.prepared.is_some() {
                    return Err(invalid("A prepared session is pending"));
                }
                let runtime = &self.runtime;
                // A new game starts the garage world. The browser reloads the page for a
                // started game or another game mode, so the garage has the new mode.
                if runtime.started
                    || runtime.stream.mode() != mode
                    || !runtime
                        .distances
                        .matches(&reference.world.draw_distance, new_game.draw_distance)
                {
                    return Err(invalid("New game needs a garage in its game mode"));
                }
                self.save.prepared = Some(FreshSessionPermit::prepare(request.id, runtime, epoch)?);
                Ok(Some((request.id.to_string(), Vec::new())))
            }
            SaveAction::Discard(_) => Err(invalid("The renderer answers Discard")),
        }
    }

    /// Writes the save results into `frame`.
    pub(crate) fn fill_saves(&mut self, frame: &mut SimFrame) {
        frame.changes.append(&mut self.save.results);
    }
}
