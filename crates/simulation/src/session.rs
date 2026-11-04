//! One simulation behind encoded commands and frames. The worker module runs it.
use crate::boundary::{DecodeError, FrameEncoder, SimInit, decode_command};
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
    sandbox: crate::sandbox::Sandbox,
}

impl Session {
    pub fn new(reference: Arc<ReferenceManifest>, init: SimInit) -> Result<Self, ReferenceError> {
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
        Ok(Self {
            reference,
            driver,
            frame: Default::default(),
            encoder: FrameEncoder::default(),
            bytes: Vec::new(),
            net_out: Vec::new(),
            load_error,
            sandbox: Default::default(),
        })
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

    /// Runs a JSON array of sandbox actions between ticks. Returns a JSON array with one reply per action.
    pub fn sandbox(&mut self, json: &str) -> String {
        self.sandbox.queued.push(json.to_owned());
        crate::sandbox::run(&mut self.driver.runtime, &self.reference, &mut self.sandbox);
        format!(
            "[{}]",
            self.sandbox.replies.drain(..).collect::<Vec<_>>().join(",")
        )
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
        let bytes = crate::reference::read_manifest(crate::reference::INSTALLED_MANIFEST).unwrap();
        let reference = Arc::new(ReferenceManifest::from_runtime(&bytes).unwrap());
        let init = SimInit {
            mode: Default::default(),
            options: Default::default(),
            draw_distance: reference.world.draw_distance.default,
            manifest_sha256: String::new(),
            save: Vec::new(),
        };
        let mut session = Session::new(reference, init).unwrap();
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
        // The renderer reads session 0 as no world yet.
        assert_ne!(frame.state.session, 0);
    }
}
