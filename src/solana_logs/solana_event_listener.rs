use tokio::sync::mpsc::UnboundedSender;

use super::{solana_retro_reader::SolanaRetroReader, EventListenerError};
use crate::solana_logs::config::SolanaListenerConfig;

pub struct SolanaEventListener {
    solana_config: SolanaListenerConfig,
    logs_retro_reader: SolanaRetroReader,
}

impl SolanaEventListener {
    pub fn new(
        solana_config: SolanaListenerConfig,
        logs_sender: UnboundedSender<LogsBunch>,
    ) -> Self {
        SolanaEventListener {
            solana_config,
            logs_retro_reader: SolanaRetroReader::new(logs_sender),
        }
    }

    pub async fn listen_to_solana(&self, tx_read_from: String) -> Result<(), EventListenerError> {
        self.logs_retro_reader.read_events_backward(self.solana_config.clone(), tx_read_from).await
    }
}

pub struct LogsBunch {
    pub need_check: bool,
    pub tx_signature: String,
    pub logs: Vec<String>,
    pub slot: u64,
}
