//! The renderer end of the simulation worker. One command is in flight; newer commands merge into the next.
use crate::browser::{game_sim_frame_length, game_sim_read, game_sim_send};
use simulation::boundary::{DecodeError, SimCommand, SimFrame, decode, encode_command};

#[derive(Default)]
pub(crate) struct SimLink {
    /// A sent command has no frame yet.
    in_flight: bool,
    /// Commands since the last send, merged.
    pending: Option<SimCommand>,
    command: Vec<u8>,
    answer: Vec<u8>,
}

impl SimLink {
    pub(crate) fn send(&mut self, command: SimCommand) {
        match &mut self.pending {
            Some(pending) => pending.merge(command),
            None => self.pending = Some(command),
        }
        self.flush();
    }

    /// Reads the frame of the command in flight into `frame`. False: no new frame.
    pub(crate) fn receive(&mut self, frame: &mut SimFrame) -> Result<bool, DecodeError> {
        if !self.in_flight {
            return Ok(false);
        }
        let Ok(length) = usize::try_from(game_sim_frame_length()) else {
            return Ok(false);
        };
        self.answer.resize(length, 0);
        game_sim_read(&mut self.answer);
        decode(&self.answer, frame)?;
        self.in_flight = false;
        self.flush();
        Ok(true)
    }

    fn flush(&mut self) {
        if self.in_flight {
            return;
        }
        let Some(command) = self.pending.take() else {
            return;
        };
        encode_command(&command, &mut self.command);
        game_sim_send(&self.command);
        self.in_flight = true;
    }
}
