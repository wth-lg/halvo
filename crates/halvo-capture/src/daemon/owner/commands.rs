//! The command path of the owner: the commands that it handles between
//! passes, each with its answer.

use std::sync::mpsc::TryRecvError;

use super::{Answer, Command, Owner};
use crate::daemon::maintenance::Clocks;
use crate::daemon::port::CaptureStore;

impl<S: CaptureStore, C: Clocks> Owner<S, C> {
    pub(super) fn handle_commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(asked) => {
                    let answer = self.handle(asked.command);
                    let _ = asked.reply.send(answer);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    if !self.stopping {
                        self.settle();
                        self.stopping = true;
                    }
                    return;
                }
            }
            if self.stopping {
                return;
            }
        }
    }

    fn handle(&mut self, command: Command) -> Answer {
        match command {
            Command::OpenPause => Answer::Paused(self.gate.open_pause(&self.writer)),
            Command::ClosePause => {
                if let Some(received) = self.inbox.take() {
                    self.take(received);
                }
                while let Ok(received) = self.writer.try_recv() {
                    self.take(received);
                }
                Answer::Resumed(self.gate.close_pause(&self.writer))
            }
            Command::Settle => Answer::Settled(self.settle()),
            Command::Shutdown => {
                let snapshot = self.settle();
                self.stopping = true;
                Answer::ShutDown(snapshot)
            }
            Command::Inventory => Answer::Inventory(self.store.inventory()),
            #[cfg(feature = "r2")]
            Command::Mark(edge) => {
                let at = self.clocks.wall();
                Answer::Marked(crate::daemon::r2::marks::handler().request(self, at, edge))
            }
        }
    }
}
