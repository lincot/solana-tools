use std::str::FromStr;
use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc::UnboundedSender;

use super::{solana_retro_reader::SolanaRetroReader, EventListenerError};
use crate::solana_logs::config::SolanaListenerConfig;

pub(crate) struct SolanaEventListener {
    solana_config: SolanaListenerConfig,
    logs_retro_reader: SolanaRetroReader,
}

impl SolanaEventListener {
    pub(crate) fn new(
        solana_config: SolanaListenerConfig,
        logs_sender: UnboundedSender<LogsBunch>,
    ) -> Self {
        SolanaEventListener {
            solana_config,
            logs_retro_reader: SolanaRetroReader::new(logs_sender),
        }
    }

    pub(crate) async fn listen_to_solana(&self) -> Result<(), EventListenerError> {
        self.logs_retro_reader.read_events_backward(&self.solana_config).await
    }
}

pub struct LogsBunch {
    pub need_check: bool,
    pub tx_signature: String,
    pub logs: Vec<String>,
    pub slot: u64,
}
