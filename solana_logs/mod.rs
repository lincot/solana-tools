use log::error;
use solana_transactor::TransactorError;
use thiserror::Error;

pub(crate) mod event_processor;
pub(crate) mod parse_logs;
pub(crate) mod solana_event_listener;
pub(crate) mod solana_retro_reader;

#[derive(Debug, Error)]
pub(crate) enum EventListenerError {
    #[error("Solana client error")]
    SolanaClient,
    #[error("Solana transactor error {0}")]
    SolanaTransacto(#[from] TransactorError),
    #[error("Mongodb client error")]
    Mongodb(#[from] mongodb::error::Error),
    #[error("Solana parse logs error")]
    SolanaParseLogs,
}
