use serde::{de::Error, Deserialize, Deserializer};
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};
use std::str::FromStr;

use crate::solana_transactor::RpcEntry;

#[derive(Clone, Debug, Deserialize)]
pub struct SolanaClientConfig {
    #[serde(deserialize_with = "deserialize_commitment")]
    pub commitment: CommitmentConfig,
    pub read_rpcs: Vec<RpcEntry>,
    pub write_rpcs: Vec<RpcEntry>,
    #[serde(deserialize_with = "deserialize_chain_id")]
    pub chain_id: u128,
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

pub fn deserialize_chain_id<'de, D>(deserializer: D) -> Result<u128, D::Error>
where D: Deserializer<'de> {
    let s = String::deserialize(deserializer)?;
    u128::from_str(&s).map_err(Error::custom)
}
