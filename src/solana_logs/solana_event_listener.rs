use solana_sdk::pubkey::Pubkey;
use tokio::sync::mpsc::UnboundedSender;

use super::{solana_retro_reader::SolanaRetroReader, EventListenerError, SolanaClientConfig};

pub struct SolanaEventListener {
    solana_client_config: SolanaClientConfig,
    program_listen_to: Pubkey,
    logs_retro_reader: SolanaRetroReader,
}

impl SolanaEventListener {
    pub fn new(
        solana_client_config: SolanaClientConfig,
        program_listen_to: Pubkey,
        logs_sender: UnboundedSender<LogsBunch>,
    ) -> Self {
        SolanaEventListener {
            solana_client_config,
            program_listen_to,
            logs_retro_reader: SolanaRetroReader::new(logs_sender),
        }
    }

    pub async fn listen_to_solana(&self, tx_read_from: String) -> Result<(), EventListenerError> {
        self.logs_retro_reader
            .read_events_backward(
                self.solana_client_config.clone(),
                self.program_listen_to,
                tx_read_from,
            )
            .await
    }
}

pub struct LogsBunch {
    pub need_check: bool,
    pub tx_signature: String,
    pub logs: Vec<String>,
    pub slot: u64,
}
