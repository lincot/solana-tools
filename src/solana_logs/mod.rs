use log::error;

use thiserror::Error;

pub(crate) mod config;
pub mod event_processor;
pub(crate) mod parse_logs;
pub mod solana_event_listener;
pub(crate) mod solana_retro_reader;

pub use config::{SolanaClientConfig, SolanaListenerConfig};
pub use event_processor::EventProcessor;
pub use solana_event_listener::LogsBunch;

use crate::solana_transactor::TransactorError;

#[derive(Debug, Error)]
pub enum EventListenerError {
    #[error("Solana client error")]
    SolanaClient,
    #[error("Solana transactor error {0}")]
    SolanaTransacto(#[from] TransactorError),
    #[error("Solana parse logs error")]
    SolanaParseLogs,
}
