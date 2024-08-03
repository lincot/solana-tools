use serde::{de::Error, Deserialize, Deserializer};
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};

use crate::solana_transactor::RpcEntry;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SolanaClientConfig {
    #[serde(deserialize_with = "deserialize_commitment")]
    pub(crate) commitment: CommitmentConfig,
    pub(crate) read_rpcs: Vec<RpcEntry>,
    pub(crate) write_rpcs: Vec<RpcEntry>,
}

fn deserialize_commitment<'de, D>(deserializer: D) -> Result<CommitmentConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let commitment = CommitmentLevel::deserialize(deserializer)
        .map_err(|err| Error::custom(format!("Malformed commitment: {}", err)))?;
    Ok(CommitmentConfig { commitment })
}

#[derive(Deserialize)]
pub(crate) struct SolanaListenerConfig {
    #[serde(flatten)]
    pub(crate) client: SolanaClientConfig,
    #[serde(alias = "txreadfrom")]
    pub(crate) tx_read_from: String,
    pub(crate) program_listen_to: String
}
