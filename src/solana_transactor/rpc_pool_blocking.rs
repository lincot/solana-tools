use solana_client::{client_error::reqwest::Url, rpc_client::RpcClient};
use solana_sdk::commitment_config::CommitmentConfig;
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, UNIX_EPOCH},
};

use super::{config::RpcEntry, round_robin_blocking::RoundRobinBlocking, TransactorError};

struct RpcBlocking {
    url: Url,
    last_accessed: AtomicU64,
    min_timeout: Duration,
    cached_version: Mutex<Option<semver::Version>>,
}

#[derive(Clone)]
pub struct RpcPoolBlocking {
    read_rpcs: RoundRobinBlocking<RpcBlocking>,
    write_rpcs: RoundRobinBlocking<RpcBlocking>,
}

impl RpcPoolBlocking {
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
                Ok(RpcBlocking {
                    url: rpc_config
                        .url
                        .parse()
                        .map_err(|_| TransactorError::InvalidRpc(rpc_config.url.clone()))?,
                    cached_version: Mutex::default(),
                    last_accessed: AtomicU64::new(now()),
                    min_timeout,
                })
            })
            .collect::<Result<Vec<RpcBlocking>, TransactorError>>()?;
        let write_rpcs = write_rpcs
            .iter()
            .map(|rpc_config| {
                let min_timeout = Duration::from_nanos(1_000_000_000 / rpc_config.ratelimit);
                Ok(RpcBlocking {
                    url: rpc_config
                        .url
                        .parse()
                        .map_err(|_| TransactorError::InvalidRpc(rpc_config.url.clone()))?,
                    cached_version: Mutex::default(),
                    last_accessed: AtomicU64::new(now()),
                    min_timeout,
                })
            })
            .collect::<Result<Vec<RpcBlocking>, TransactorError>>()?;
        Ok(Self {
            read_rpcs: RoundRobinBlocking::new(read_rpcs),
            write_rpcs: RoundRobinBlocking::new(write_rpcs),
        })
    }

    pub fn with_read_rpc<F, T>(&self, f: F, commitment: CommitmentConfig) -> T
    where F: FnOnce(RpcClient) -> T {
        let _now = now();
        let (rpc, elapsed) = self
            .read_rpcs
            .pull_by_max(|x| _now - x.last_accessed.load(Ordering::Acquire))
            .expect("Empty round robin pool");
        if elapsed < rpc.min_timeout.as_millis() as u64 {
            std::thread::sleep(Duration::from_millis(rpc.min_timeout.as_millis() as u64 - elapsed));
        }
        let rpc_version = rpc.cached_version.lock().expect("Failed to lock rpc_version");
        let client = RpcClient::new_with_timeout_and_commitment(
            rpc.url.to_string(),
            Duration::from_secs(3),
            commitment,
        );
        let res = f(client);
        rpc.last_accessed.store(now(), Ordering::Release);
        // rpc_version should be locked until `f` has completed
        drop(rpc_version);
        res
    }

    pub fn with_write_rpc<F, T>(&self, f: F, commitment: CommitmentConfig) -> T
    where F: FnOnce(RpcClient) -> T {
        let _now = now();
        let (rpc, elapsed) = self
            .write_rpcs
            .pull_by_max(|x| _now - x.last_accessed.load(Ordering::Acquire))
            .expect("Empty round robin pool");
        if elapsed < rpc.min_timeout.as_millis() as u64 {
            std::thread::sleep(Duration::from_millis(rpc.min_timeout.as_millis() as u64 - elapsed));
        }
        let rpc_version = rpc.cached_version.lock().expect("Failed to lock rpc.cached_version");
        let client = RpcClient::new_with_timeout_and_commitment(
            rpc.url.to_string(),
            Duration::from_secs(3),
            commitment,
        );
        let res = f(client);
        rpc.last_accessed.store(now(), Ordering::Release);
        // rpc_version should be locked until `f` has completed
        drop(rpc_version);
        res
    }

    pub fn with_read_rpc_loop<F, O, E>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: Fn(RpcClient) -> Result<O, E> + std::clone::Clone,
        E: Debug,
    {
        let mut i = 0;
        let mut x = 0;
        loop {
            match self.with_read_rpc(f.clone(), commitment) {
                Ok(x) => break x,
                Err(_e) => {
                    // log::warn!("RPC error: {:?}", e);
                    i += 1;
                    let n = self.read_rpcs.len() as u64;
                    if i % n == 0 {
                        let to_wait = x * 6 + n * 3;
                        x += 1;
                        // log::warn!("RPC pool exhausted ({} times), waiting {}s", x, to_wait);
                        std::thread::sleep(Duration::from_secs(to_wait));
                    }
                }
            }
        }
    }

    pub fn with_write_rpc_loop<F, O, E>(&self, f: F, commitment: CommitmentConfig) -> O
    where
        F: Fn(RpcClient) -> Result<O, E> + Clone,
        E: Debug,
    {
        let mut i = 0;
        let mut x = 0;
        loop {
            match self.with_write_rpc(f.clone(), commitment) {
                Ok(x) => break x,
                Err(_e) => {
                    // log::warn!("RPC error: {:?}", e);
                    i += 1;
                    let n = self.write_rpcs.len() as u64;
                    if i % n == 0 {
                        let to_wait = x * 6 + n * 3;
                        x += 1;
                        // log::warn!("RPC pool exhausted ({} times), waiting {}s", x, to_wait);
                        std::thread::sleep(Duration::from_secs(to_wait));
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

#[cfg(test)]
mod test {
    use solana_client::client_error::ClientError;

    use super::*;

    #[test]
    fn test_rpc_pool_blocking() {
        let rpcs = &[RpcEntry {
            url: "https://api.devnet.solana.com".to_string(),
            ratelimit: 1,
        }];
        let rpc_pool = RpcPoolBlocking::new(rpcs, rpcs).unwrap();
        rpc_pool.with_read_rpc_loop(
            |rpc| {
                println!("Block height {}", rpc.get_block_height()?);
                Ok::<(), ClientError>(())
            },
            CommitmentConfig::confirmed(),
        );
        rpc_pool.with_read_rpc_loop(
            |rpc| {
                println!("Slot {}", rpc.get_slot()?);
                Ok::<(), ClientError>(())
            },
            CommitmentConfig::confirmed(),
        );
    }
}
