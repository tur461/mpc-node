use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ShareGeneration { threshold: usize },
    SignMessage { message: Vec<u8> },
    UpdateState { key: String, value: Vec<u8> },
    ConsensusProposal { round: u64, value: Vec<u8> },
}

pub struct CommandProcessor {
    command_tx: mpsc::Sender<Command>,
    command_rx: mpsc::Receiver<Command>,
    handlers: HashMap<String, Box<dyn Fn(Command) -> anyhow::Result<()> + Send + Sync>>,
}

impl CommandProcessor {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            command_tx: tx,
            command_rx: rx,
            handlers: HashMap::new(),
        }
    }

    pub fn register_handler<F>(&mut self, command_type: String, handler: F)
    where
        F: Fn(Command) -> anyhow::Result<()> + Send + Sync + 'static,
    {
        self.handlers.insert(command_type, Box::new(handler));
    }

    pub async fn handle_message(&mut self, msg: Command) -> anyhow::Result<()> {
        match msg {
            Command::ShareGeneration { threshold} => {
                if let Some(handler) = self.handlers.get("share_generation") {
                    handler(Command::ShareGeneration { threshold })?;
                }
            }
            Command::SignMessage { message } => {
                if let Some(handler) = self.handlers.get("sign_message") {
                    handler(Command::SignMessage { message })?;
                }
            }
            Command::UpdateState { key, value } => {
                if let Some(handler) = self.handlers.get("update_state") {
                    handler(Command::UpdateState { key, value })?;
                }
            },
            Command::ConsensusProposal { round, value } => {
                if let Some(handler) = self.handlers.get("consensus_proposal") {
                    handler(Command::ConsensusProposal { round, value })?;
                }
            },
        }
        Ok(())
    }
}