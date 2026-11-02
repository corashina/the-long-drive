//! One simulation behind encoded commands and frames. The worker module runs it.
use crate::boundary::{DecodeError, FrameEncoder, SimInit, SimStartup, decode_command};
use crate::driver::SimDriver;
use crate::reference::{ReferenceError, ReferenceManifest};
use crate::save::{SaveError, SaveSnapshot, SourceStamp};
use crate::starting_area::GarageRuntime;
use std::sync::Arc;

pub struct Session {
    reference: Arc<ReferenceManifest>,
    driver: SimDriver,
    /// The frame of the last tick. The next tick fills it again.
    frame: crate::boundary::SimFrame,
    encoder: FrameEncoder,
    bytes: Vec<u8>,
    net_out: Vec<u8>,
    /// The save of `SimInit` did not load. The garage of a new game starts.
    load_error: Option<String>,
}

impl Session {
    pub fn new(
        reference: Arc<ReferenceManifest>,
        init: SimInit,
    ) -> Result<(Self, SimStartup), ReferenceError> {
        let source = SourceStamp::new(&reference, init.manifest_sha256);
        let mut load_error = None;
        let saved = (!init.save.is_empty()).then(|| {
            let saved = serde_json::from_slice::<SaveSnapshot>(&init.save)
                .map_err(|e| SaveError::CorruptData(e.to_string()))?;
            let mut runtime = GarageRuntime::from_save(
                &reference,
                &source,
                &saved,
                init.mode,
                init.draw_distance,
            )?;
            runtime.started = true;
            runtime.paused = true;
            Ok::<_, SaveError>(runtime)
        });
        let mut runtime = match saved {
            Some(Ok(runtime)) => runtime,
            other => {
                load_error = other.and_then(Result::err).map(|e| e.to_string());
                GarageRuntime::new(&reference, init.mode, init.draw_distance)
                    .map_err(|e| ReferenceError(e.to_string()))?
            }
        };
        runtime.set_game_options(init.options);
        let driver = SimDriver::new(runtime, source);
        let startup = driver.startup();
        let session = Self {
            reference,
            driver,
            frame: Default::default(),
            encoder: FrameEncoder::default(),
            bytes: Vec::new(),
            net_out: Vec::new(),
            load_error,
        };
        Ok((session, startup))
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Starts or ends a multiplayer session. `role`: 0 off, 1 host, 2 guest.
    pub fn net_session(&mut self, role: u8, slot: u8) {
        use crate::net::NetRole;
        let role = match role {
            1 => NetRole::Host,
            2 => NetRole::Guest,
            _ => NetRole::Off,
        };
        self.driver.net.set_session(role, slot);
    }
    /// Length-prefixed packets from peers, for the next tick.
    pub fn net_receive(&mut self, packets: &[u8]) {
        self.driver.net.receive(packets);
    }
    pub fn net_leave(&mut self, slot: u8) {
        self.driver.net.leave(slot);
    }
    /// Length-prefixed packets of the last tick, for peers.
    pub fn net_send(&mut self) -> &[u8] {
        self.driver.net.take_outbox(&mut self.net_out);
        &self.net_out
    }

    /// Runs one encoded command. Returns the encoded frame.
    pub fn tick(&mut self, command: &[u8]) -> Result<&[u8], DecodeError> {
        let command = decode_command(command)?;
        self.driver.tick(&self.reference, command, &mut self.frame);
        self.encoder.encode(&self.frame, &mut self.bytes);
        Ok(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::{SimCommand, SimFrame, decode, encode_command};

    #[test]
    #[ignore = "requires the installed source export"]
    fn tick_answers_a_decodable_frame() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap();
        let reference = Arc::new(ReferenceManifest::from_runtime_json(&bytes).unwrap());
        let init = SimInit {
            mode: Default::default(),
            options: Default::default(),
            draw_distance: reference.world.draw_distance.default,
            manifest_sha256: String::new(),
            save: Vec::new(),
        };
        let (mut session, startup) = Session::new(reference, init).unwrap();
        let mut command = Vec::new();
        encode_command(
            &SimCommand {
                seq: 1,
                delta_seconds: 1. / 60.,
                ..Default::default()
            },
            &mut command,
        );
        let mut frame = SimFrame::default();
        decode(session.tick(&command).unwrap(), &mut frame).unwrap();
        assert_eq!(frame.seq_consumed, 1);
        assert_eq!(frame.state.session, startup.session);
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn sandbox_commands_answer_in_the_frame() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap();
        let reference = Arc::new(ReferenceManifest::from_runtime_json(&bytes).unwrap());
        let item = reference
            .item_prototypes
            .iter()
            .find_map(|p| {
                let id = p.item_id?;
                reference.world.catalog.items.iter().find(|c| c.index == id)
            })
            .unwrap()
            .name
            .clone();
        let init = SimInit {
            mode: Default::default(),
            options: Default::default(),
            draw_distance: reference.world.draw_distance.default,
            manifest_sha256: String::new(),
            save: Vec::new(),
        };
        let (mut session, _) = Session::new(reference, init).unwrap();
        let sandbox = serde_json::json!([
            {"id": 1, "action": "garage"},
            {"id": 2, "action": "spawn", "name": item, "forward": 2.0},
            {"id": 3, "action": "cycle", "seconds": 1.0},
            {"id": 4, "action": "spawn", "name": "no such item", "forward": 2.0},
            {"id": 5, "action": "state", "radius": 10.0},
        ]);
        let mut command = Vec::new();
        encode_command(
            &SimCommand {
                seq: 1,
                delta_seconds: 1. / 60.,
                sandbox: vec![sandbox.to_string()],
                ..Default::default()
            },
            &mut command,
        );
        let mut frame = SimFrame::default();
        decode(session.tick(&command).unwrap(), &mut frame).unwrap();
        let replies: Vec<serde_json::Value> = frame
            .changes
            .iter()
            .filter_map(|c| match c {
                crate::boundary::Change::Sandbox(json) => serde_json::from_str(json).ok(),
                _ => None,
            })
            .collect();
        let ok: Vec<_> = replies.iter().map(|r| r["ok"].as_bool().unwrap()).collect();
        assert_eq!(ok, [true, true, true, false, true], "{replies:?}");
        assert!(!replies[4]["data"]["items"].as_array().unwrap().is_empty());
    }
}
