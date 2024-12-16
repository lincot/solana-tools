use solana_client::{client_error::reqwest::Url, nonblocking::rpc_client::RpcClient};
use solana_sdk::commitment_config::CommitmentConfig;
use std::{
    fmt::Debug,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, UNIX_EPOCH},
};

use super::{config::RpcEntry, round_robin::RoundRobin, TransactorError};

struct Rpc {
    url: Url,
    last_accessed: AtomicU64,
    min_timeout: Duration,
}

#[derive(Clone)]
pub struct RpcPool {
    read_rpcs: RoundRobin<Rpc>,
    write_rpcs: RoundRobin<Rpc>,
}

impl RpcPool {
    pub fn new(read_rpcs: &[RpcEntry], write_rpcs: &[RpcEntry]) -> Result<Self, TransactorError> {
        if read_rpcs.is_empty() {
            return Err(TransactorError::NoReadRpcs);
        }
        if write_rpcs.is_empty() {
            return Err(TransactorError::NoWriteRpcs);
        }
        let read_rpcs = read_rpcs
            .iter()
            .map(|rpc_config| {
                let min_timeout = Duration::from_nanos(1_000_000_000 / rpc_config.ratelimit);
                Ok(Rpc {
                    url: rpc_config
                        .url
                        .parse()
                        .map_err(|_| TransactorError::InvalidRpc(rpc_config.url.clone()))?,
                    last_accessed: AtomicU64::new(now()),
                    min_timeout,
                })
            })
            .collect::<Result<Vec<Rpc>, TransactorError>>()?;
        let write_rpcs = write_rpcs
            .iter()
            .map(|rpc_config| {
                let min_timeout = Duration::from_nanos(1_000_000_000 / rpc_config.ratelimit);
                Ok(Rpc {
                    url: rpc_config
                        .url
                        .parse()
                        .map_err(|_| TransactorError::InvalidRpc(rpc_config.url.clone()))?,
                    last_accessed: AtomicU64::new(now()),
                    min_timeout,
                })
            })
            .collect::<Result<Vec<Rpc>, TransactorError>>()?;
        Ok(Self {
            read_rpcs: RoundRobin::new(read_rpcs),
            write_rpcs: RoundRobin::new(write_rpcs),
        })
    }

    pub async fn with_read_rpc<F, T, O>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: FnOnce(RpcClient) -> T,
        T: std::future::Future<Output = O>,
    {
        let _now = now();
        let (rpc, elapsed) = self
            .read_rpcs
            .pull_by_max(|x| _now - x.last_accessed.load(Ordering::Acquire))
            .await
            .expect("Empty round robin pool");
        if elapsed < rpc.min_timeout.as_millis() as u64 {
            tokio::time::sleep(Duration::from_millis(rpc.min_timeout.as_millis() as u64 - elapsed))
                .await;
        }
        let client = RpcClient::new_with_timeout_and_commitment(
            rpc.url.to_string(),
            Duration::from_secs(3),
            commitment,
        );
        let res = f(client).await;
        rpc.last_accessed.store(now(), Ordering::Release);
        res
    }

    pub async fn with_write_rpc<F, T, O>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: FnOnce(RpcClient) -> T,
        T: std::future::Future<Output = O>,
    {
        let _now = now();
        let (rpc, elapsed) = self
            .write_rpcs
            .pull_by_max(|x| _now - x.last_accessed.load(Ordering::Acquire))
            .await
            .expect("Empty round robin pool");
        if elapsed < rpc.min_timeout.as_millis() as u64 {
            tokio::time::sleep(Duration::from_millis(rpc.min_timeout.as_millis() as u64 - elapsed))
                .await;
        }
        let client = RpcClient::new_with_timeout_and_commitment(
            rpc.url.to_string(),
            Duration::from_secs(3),
            commitment,
        );
        let res = f(client).await;
        rpc.last_accessed.store(now(), Ordering::Release);
        res
    }

    pub async fn with_read_rpc_loop<F, T, O, E>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: Fn(RpcClient) -> T + Clone,
        T: std::future::Future<Output = Result<O, E>>,
        E: Debug,
    {
        let mut i = 0;
        let mut x = 0;
        loop {
            match self.with_read_rpc(f.clone(), commitment).await {
                Ok(x) => break x,
                Err(_e) => {
                    // log::warn!("RPC error: {:?}", e);
                    i += 1;
                    let n = self.read_rpcs.len() as u64;
                    if i % n == 0 {
                        let to_wait = x * 6 + n * 3;
                        x += 1;
                        // log::warn!("RPC pool exhausted ({} times), waiting {}s", x, to_wait);
                        tokio::time::sleep(Duration::from_secs(to_wait)).await
                    }
                }
            }
        }
    }

    pub async fn with_write_rpc_loop<F, T, O, E>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: Fn(RpcClient) -> T + Clone,
        T: std::future::Future<Output = Result<O, E>>,
        E: Debug,
    {
        let mut i = 0;
        let mut x = 0;
        loop {
            match self.with_write_rpc(f.clone(), commitment).await {
                Ok(x) => break x,
                Err(_e) => {
                    // log::warn!("RPC error: {:?}", e);
                    i += 1;
                    let n = self.write_rpcs.len() as u64;
                    if i % n == 0 {
                        let to_wait = x * 6 + n * 3;
                        x += 1;
                        // log::warn!("RPC pool exhausted ({} times), waiting {}s", x, to_wait);
                        tokio::time::sleep(Duration::from_secs(to_wait)).await
                    }
                }
            }
        }
    }

    pub fn num_read_rpcs(&self) -> usize {
        self.read_rpcs.len()
    }

    pub fn num_write_rpcs(&self) -> usize {
        self.write_rpcs.len()
    }
}

fn now() -> u64 {
    UNIX_EPOCH.elapsed().expect("Get time failed").as_millis() as u64
}
