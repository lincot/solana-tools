#[macro_use]

pub mod alt_manager;
mod config;
mod error;
pub mod ix_compiler;
mod round_robin;
mod rpc_pool;
mod transactor;

pub use config::*;
pub use error::TransactorError;
pub use round_robin::RoundRobin;
pub use rpc_pool::RpcPool;
pub use transactor::*;

pub type ExecutorPool = RoundRobin<solana_sdk::signer::keypair::Keypair>;

#[macro_export]
macro_rules! log_with_ctx {
    ($level:ident, $log_ctx:expr, $($arg:tt)*) => {
        match $log_ctx {
            Some(ref ctx) => log::$level!("{}. {}", ctx, format!($($arg)*)),
            None => log::$level!($($arg)*),
        }
    };
}
