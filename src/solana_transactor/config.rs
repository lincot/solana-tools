use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEntry {
    /// RPC address
    pub url: String,
    /// Rate limit in requests per second
    pub ratelimit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaTransactorConfig {
    pub read_pool: Vec<RpcEntry>,
    pub write_pool: Vec<RpcEntry>,
}
