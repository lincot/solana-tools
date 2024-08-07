use serde::{de::Error, Deserialize, Deserializer};
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};

use crate::solana_transactor::RpcEntry;

#[derive(Clone, Debug, Deserialize)]
pub struct SolanaClientConfig {
    #[serde(deserialize_with = "deserialize_commitment")]
    pub commitment: CommitmentConfig,
    pub read_rpcs: Vec<RpcEntry>,
    pub write_rpcs: Vec<RpcEntry>,
}

fn deserialize_commitment<'de, D>(deserializer: D) -> Result<CommitmentConfig, D::Error>
where D: Deserializer<'de> {
    let commitment = CommitmentLevel::deserialize(deserializer)
        .map_err(|err| Error::custom(format!("Malformed commitment: {}", err)))?;
    Ok(CommitmentConfig { commitment })
}

#[derive(Clone, Debug, Deserialize)]
pub struct SolanaListenerConfig {
    #[serde(flatten)]
    pub client: SolanaClientConfig,
    #[serde(alias = "txreadfrom")]
    pub tx_read_from: String,
    pub tx_read_from_force: Option<String>,
    pub program_listen_to: String,
}
