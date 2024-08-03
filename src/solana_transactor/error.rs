use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransactorError {
    #[error("No read rpcs provided")]
    NoReadRpcs,
    #[error("No write rpcs provided")]
    NoWriteRpcs,
    #[error("Invalid Solana RPC provided {0}")]
    InvalidRpc(String),
    #[error("Transaction bundle is empty")]
    EmptyBundle,
    #[error("Client error {0}")]
    ClientError(#[from] solana_client::client_error::ClientError),
    #[error("Instruction error {0}")]
    InstructionError(#[from] solana_sdk::instruction::InstructionError),
    #[error("Transaction partial sign failed {0}")]
    FailedToSign(#[from] solana_sdk::signer::SignerError),
    #[error("Failed to compile message {0}")]
    FailedToCompile(#[from] solana_sdk::message::CompileError),
    #[error("Instruction too big")]
    InstructionTooBig,
}
