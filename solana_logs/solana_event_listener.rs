use tokio::sync::mpsc::UnboundedSender;

use transmitter_common::mongodb::MongodbConfig;

use super::{solana_retro_reader::SolanaRetroReader, EventListenerError};
use crate::common::config::SolanaListenerConfig;

pub(crate) struct SolanaEventListener {
    solana_config: SolanaListenerConfig,
    mongodb_config: MongodbConfig,
    logs_retro_reader: SolanaRetroReader,
}

impl SolanaEventListener {
    pub(crate) fn new(
        solana_config: SolanaListenerConfig,
        mongodb_config: MongodbConfig,
        logs_sender: UnboundedSender<LogsBunch>,
    ) -> Self {
        SolanaEventListener {
            solana_config,
            mongodb_config: mongodb_config.clone(),
            logs_retro_reader: SolanaRetroReader::new(mongodb_config, logs_sender),
        }
    }

    pub(crate) async fn listen_to_solana(&self) -> Result<(), EventListenerError> {
        self.logs_retro_reader.read_events_backward(&self.solana_config, &self.mongodb_config).await
    }
}

pub(crate) struct LogsBunch {
    pub need_check: bool,
    pub tx_signature: String,
    pub logs: Vec<String>,
    pub slot: u64,
}
