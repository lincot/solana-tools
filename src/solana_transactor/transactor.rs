#![allow(clippy::large_enum_variant)]
#![allow(clippy::too_many_arguments)]

use futures::{StreamExt, TryStreamExt};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    message::VersionedMessage,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    transaction::{self, VersionedTransaction},
};
use std::{
    fmt::Display,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        Mutex,
    },
    task::JoinHandle,
};
use uuid::Uuid;

use super::{
    ix_compiler::{InstructionBundle, IxCompiler},
    rpc_pool::RpcPool,
    TransactorError,
};

use crate::log_with_ctx;

/// The timeframe in which we check the status of a transaction before sending
/// a new one. The reason we chose 16 seconds is that, on Solana Test Validator
/// 2.1.0, it can take up to 15 seconds to confirm a transaction that uses ALTs.
const TX_STATUS_POLL_TIMEOUT: Duration = Duration::from_secs(16);
const TX_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const STALE_SIGNATURE_THRESHOLD: Duration = Duration::from_secs(30);
const BLOCKHASH_REFRESH_INTERVAL: Duration = Duration::from_millis(1_100);
const TX_FINALIZE_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct MessageBundle {
    pub message: VersionedMessage,
    pub signers: Arc<Vec<Keypair>>,
    pub payer: Pubkey,
}

impl MessageBundle {
    pub fn new(message: &VersionedMessage, signers: &[&Keypair], payer: Pubkey) -> Self {
        Self {
            message: message.to_owned(),
            signers: Arc::new(
                signers
                    .iter()
                    .map(|x| Keypair::from_bytes(&x.to_bytes()).expect("Always 64 bytes"))
                    .collect(),
            ),
            payer,
        }
    }
}

enum ChannelMessage {
    Task(FinalizationTask),
    Stop,
}

struct FinalizationTask {
    log_ctx: Option<String>,
    signature: Signature,
    bundle: MessageBundle,
    id: Uuid,
    start: Instant,
}

#[derive(Clone)]
pub struct TxResult {
    pub signature: Signature,
    pub status: transaction::Result<()>,
}

#[derive(Clone)]
pub struct SolanaTransactor {
    rpc_pool: RpcPool,
    finalize_channel: Arc<UnboundedSender<ChannelMessage>>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl SolanaTransactor {
    pub async fn start(rpc_pool: RpcPool) -> Result<Self, TransactorError> {
        let (sender, receiver) = unbounded_channel();
        let s = Self {
            rpc_pool,
            finalize_channel: Arc::new(sender),
            handle: Default::default(),
        };
        let s2 = s.clone();
        let handle = tokio::task::spawn(async move { s2.run_finalizer_loop(receiver).await });
        *s.handle.lock().await = Some(handle);
        Ok(s)
    }

    pub fn rpc_pool(&self) -> &RpcPool {
        &self.rpc_pool
    }

    pub async fn get_blockhash(&self) -> Hash {
        self.rpc_pool
            .with_read_rpc_loop(
                |rpc| async move { rpc.get_latest_blockhash().await.map_err(|e| (e, rpc.url())) },
                CommitmentConfig::confirmed(),
            )
            .await
    }

    pub async fn check_account_exists(&self, addr: &Pubkey) -> bool {
        self.rpc_pool
            .with_read_rpc_loop(
                |rpc| async move { rpc.get_balance(addr).await },
                CommitmentConfig::confirmed(),
            )
            .await
            > 0
    }

    async fn get_tx_status(
        &self,
        signature: &Signature,
        commitment: CommitmentConfig,
    ) -> Option<transaction::Result<()>> {
        loop {
            match self
                .rpc_pool
                .with_read_rpc(
                    |rpc| async move {
                        rpc.get_signature_status_with_commitment(signature, commitment)
                            .await
                            .map_err(|e| (e, rpc.url()))
                    },
                    commitment,
                )
                .await
            {
                Ok(status) => {
                    return status;
                }
                Err((e, url)) => {
                    log::warn!("Failed to check tx status: {} ({})", e, url);
                }
            }
        }
    }

    async fn send_with_level_confirmed<T: Display>(
        &self,
        log_ctx: Option<T>,
        bundle: &MessageBundle,
        id: Uuid,
    ) -> Result<TxResult, TransactorError> {
        let mut current_blockhash = self.get_blockhash().await;
        let mut queue = Vec::new();
        let mut queue_start = 0;
        let bundle_start = Instant::now();
        loop {
            let signers_ref: Vec<_> = bundle.signers.iter().collect();
            let mut msg = bundle.message.clone();
            msg.set_recent_blockhash(current_blockhash);
            let tx = VersionedTransaction::try_new(msg, &signers_ref)
                .map_err(TransactorError::FailedToSign)?;
            let signature = self.send_transaction(&log_ctx, tx).await;
            let tx_status_loop_start = Instant::now();
            queue.push((signature, tx_status_loop_start));
            log_with_ctx!(
                debug,
                log_ctx,
                "Sent bundle {} with sig {}, awaiting {}",
                id,
                signature,
                queue.len()
            );
            if let Some(status) = self
                .wait_for_confirm(
                    &log_ctx,
                    id,
                    &queue,
                    &mut queue_start,
                    bundle_start,
                    tx_status_loop_start,
                )
                .await
            {
                return status;
            }
            self.get_new_blockhash(&mut current_blockhash).await;
        }
    }

    async fn get_new_blockhash(&self, current_blockhash: &mut Hash) {
        loop {
            let new_blockhash = self.get_blockhash().await;
            if new_blockhash != *current_blockhash {
                *current_blockhash = new_blockhash;
                break;
            } else {
                tokio::time::sleep(BLOCKHASH_REFRESH_INTERVAL).await;
            }
        }
    }

    async fn wait_for_confirm<T: Display>(
        &self,
        log_ctx: &Option<T>,
        id: Uuid,
        queue: &[(Signature, Instant)],
        queue_start: &mut usize,
        bundle_start: Instant,
        tx_status_loop_start: Instant,
    ) -> Option<Result<TxResult, TransactorError>> {
        while tx_status_loop_start.elapsed() < TX_STATUS_POLL_TIMEOUT {
            tokio::time::sleep(TX_STATUS_POLL_INTERVAL).await;
            for &(signature, sig_start) in &queue[*queue_start..] {
                if sig_start.elapsed() > STALE_SIGNATURE_THRESHOLD {
                    *queue_start += 1;
                    continue;
                }
                if let Some(status) =
                    self.get_tx_status(&signature, CommitmentConfig::confirmed()).await
                {
                    log_with_ctx!(
                        debug,
                        log_ctx,
                        "Bundle: {}, confirmed sig: {}, after: {}s, status: {:?}, finalizing...",
                        id,
                        signature,
                        bundle_start.elapsed().as_secs(),
                        status,
                    );
                    return Some(Ok(TxResult { signature, status }));
                }
            }
        }
        None
    }

    async fn send_transaction<T: Display>(
        &self,
        log_ctx: &Option<T>,
        tx: VersionedTransaction,
    ) -> Signature {
        loop {
            let tx = tx.clone();
            match self
                .rpc_pool
                .with_write_rpc(
                    |rpc| async move {
                        rpc.send_transaction_with_config(
                            &tx,
                            RpcSendTransactionConfig {
                                skip_preflight: true,
                                ..Default::default()
                            },
                        )
                        .await
                        .map_err(|e| (e, rpc.url()))
                    },
                    CommitmentConfig::confirmed(),
                )
                .await
            {
                Ok(s) => {
                    break s;
                }
                Err((e, url)) => {
                    log_with_ctx!(warn, log_ctx, "Failed to send tx: {} ({})", e, url);
                }
            }
        }
    }

    async fn finalize(
        &self,
        log_ctx: Option<&str>,
        signature: &Signature,
        bundle: MessageBundle,
        id: Uuid,
        start: Instant,
    ) -> Result<(), TransactorError> {
        for _ in 0..20 {
            tokio::time::sleep(TX_FINALIZE_POLL_INTERVAL).await;
            match self
                .rpc_pool
                .with_read_rpc(
                    |rpc| async move {
                        rpc.get_signature_status_with_commitment(
                            signature,
                            CommitmentConfig::finalized(),
                        )
                        .await
                        .map_err(|e| (e, rpc.url()))
                    },
                    CommitmentConfig::finalized(),
                )
                .await
            {
                Ok(Some(_)) => {
                    log_with_ctx!(
                        debug,
                        log_ctx,
                        "Bundle {} finalized {} after {} s",
                        id,
                        signature,
                        start.elapsed().as_secs()
                    );
                    return Ok(());
                }
                Err((e, url)) => {
                    log::warn!("Failed to check tx status: {} ({})", e, url);
                }
                Ok(None) => {}
            }
        }
        log_with_ctx!(warn, log_ctx, "Failed to finalize bundle {} tx {}", id, signature);
        let c = self.finalize_channel.clone();
        let _ = self.send_bundle(log_ctx, bundle, id, start, true, c).await?;
        Ok(())
    }

    async fn run_finalizer_loop(&self, mut receiver: UnboundedReceiver<ChannelMessage>) {
        while let Some(msg) = receiver.recv().await {
            match msg {
                ChannelMessage::Task(task) => {
                    self.finalize(
                        task.log_ctx.as_deref(),
                        &task.signature,
                        task.bundle,
                        task.id,
                        task.start,
                    )
                    .await
                    .expect("Finalize should not fail");
                }
                ChannelMessage::Stop => break,
            }
        }
    }

    async fn send_bundle<T: Display + Clone>(
        &self,
        log_ctx: Option<T>,
        bundle: MessageBundle,
        id: Uuid,
        start: Instant,
        finalize: bool,
        finalize_channel: Arc<UnboundedSender<ChannelMessage>>,
    ) -> Result<TxResult, TransactorError> {
        let res = self.send_with_level_confirmed(log_ctx.clone(), &bundle, id).await?;
        if finalize {
            finalize_channel
                .send(ChannelMessage::Task(FinalizationTask {
                    log_ctx: log_ctx.map(|c| c.to_string()),
                    signature: res.signature,
                    bundle,
                    id,
                    start,
                }))
                .expect("Channel error");
        }
        Ok(res)
    }

    pub async fn send<T: Display + Clone>(
        &self,
        log_ctx: Option<T>,
        bundles: &[MessageBundle],
        finalize: bool,
    ) -> Result<Vec<TxResult>, TransactorError> {
        futures::stream::iter(bundles)
            .then(|bundle| {
                let log_ctx = log_ctx.clone();
                let bundle = bundle.clone();
                let id = Uuid::new_v4();
                let start = Instant::now();
                let finalize_channel = self.finalize_channel.clone();

                async move {
                    self.send_bundle(log_ctx, bundle, id, start, finalize, finalize_channel).await
                }
            })
            .try_collect()
            .await
    }

    pub async fn send_all_instructions<T: Display + Clone>(
        &self,
        log_ctx: Option<T>,
        instructions: &[InstructionBundle],
        signers: &[&Keypair],
        payer: Pubkey,
        parallel_limit: usize,
        compute_unit_price: Option<u64>,
        finalize: bool,
    ) -> Result<Vec<TxResult>, TransactorError> {
        let mut ix_compiler = IxCompiler::new(payer, compute_unit_price);
        let messages: Result<Vec<_>, TransactorError> = instructions
            .iter()
            .filter_map(|ix| {
                ix_compiler
                    .compile(
                        log_ctx.clone(),
                        ix.instruction.clone(),
                        &ix.alt_accounts,
                        ix.compute_units,
                        ix.heap_frame,
                    )
                    .transpose()
            })
            .collect();
        let mut messages = messages?;
        if let Some(msg) = ix_compiler.flush()? {
            messages.push(msg);
        }

        futures::stream::iter(messages.into_iter().map(|msg| {
            let log_ctx = log_ctx.clone();
            async move {
                let bundle = MessageBundle::new(&msg, signers, payer);
                self.send(log_ctx, &[bundle], finalize)
                    .await
                    .map(|v| v.into_iter().next().expect("Expected \"send\" to contain one result"))
            }
        }))
        .buffer_unordered(parallel_limit)
        .try_collect()
        .await
    }

    pub async fn await_all_tx(&self) {
        if let Some(handle) = self.handle.lock().await.take() {
            self.finalize_channel.send(ChannelMessage::Stop).expect("Channel error");
            self.finalize_channel.closed().await;
            handle.await.expect("Await handle error");
        }
    }
}
