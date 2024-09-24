use log::{debug, error};

use crate::solana_transactor::RpcPool;
use solana_client::{
    rpc_client::GetConfirmedSignaturesForAddress2Config, rpc_config::RpcTransactionConfig,
    rpc_response::RpcConfirmedTransactionStatusWithSignature,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::UiTransactionEncoding;
use std::{collections::VecDeque, str::FromStr, time::Duration};
use tokio::sync::mpsc::UnboundedSender;

use crate::solana_logs::{
    config::{SolanaClientConfig, SolanaListenerConfig},
    EventListenerError, LogsBunch,
};

pub(super) struct SolanaRetroReader {
    logs_sender: UnboundedSender<LogsBunch>,
}

impl SolanaRetroReader {
    pub(super) fn new(logs_sender: UnboundedSender<LogsBunch>) -> SolanaRetroReader {
        SolanaRetroReader { logs_sender }
    }

    pub(super) async fn read_events_backward(
        &self,
        solana_config: SolanaListenerConfig,
        tx_read_from: String,
    ) -> Result<(), EventListenerError> {
        let program_to_listen =
            Pubkey::from_str(&solana_config.program_listen_to).expect("Expected to be");

        debug!("Found tx_read_from, start backward reading until: {}", tx_read_from);
        let rpc_pool =
            RpcPool::new(&solana_config.client.read_rpcs, &solana_config.client.write_rpcs)?;

        let mut tx_read_from = Some(Signature::from_str(&tx_read_from).map_err(|err| {
            error!("Failed to decode tx_start_from: {}", err);
            EventListenerError::SolanaClient
        })?);
        let mut until: Option<Signature> = None;
        let mut need_check: bool;
        let mut next_until: Option<Signature> = None;
        loop {
            let mut before = None;
            let mut log_bunches = VecDeque::new();
            (until, need_check) = if tx_read_from.is_some() {
                (tx_read_from.take(), true)
            } else if next_until.is_some() {
                (next_until, false)
            } else {
                (until, false)
            };
            next_until = None;
            loop {
                let signatures_backward = Self::get_signatures_chunk(
                    &program_to_listen,
                    &solana_config.client,
                    &rpc_pool,
                    until,
                    before,
                )
                .await?;

                if signatures_backward.is_empty() {
                    break;
                }
                if next_until.is_none() {
                    next_until = Signature::from_str(
                        &signatures_backward
                            .first()
                            .expect("The newest tx expected to be")
                            .signature,
                    )
                    .unwrap()
                    .into()
                }
                Self::process_signatures(
                    &rpc_pool,
                    &mut before,
                    &mut log_bunches,
                    signatures_backward,
                    solana_config.client.commitment,
                    need_check,
                )
                .await;
            }
            if !log_bunches.is_empty() {
                debug!("Logs bunch have gotten: {}", log_bunches.len());
            }
            next_until = log_bunches.back().map(|b| Signature::from_str(&b.tx_signature).unwrap());
            for logs_bunch in log_bunches {
                self.logs_sender.send(logs_bunch).expect("Expected logs_bunch to be sent");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    async fn process_signatures(
        rpc_pool: &RpcPool,
        before: &mut Option<Signature>,
        log_bunches: &mut VecDeque<LogsBunch>,
        signatures_with_meta: Vec<RpcConfirmedTransactionStatusWithSignature>,
        commitment: CommitmentConfig,
        need_check: bool,
    ) {
        for signature_with_meta in signatures_with_meta {
            _ = Self::process_signature(
                rpc_pool,
                before,
                log_bunches,
                signature_with_meta,
                commitment,
                need_check,
            )
            .await;
        }
    }

    async fn process_signature(
        rpc_pool: &RpcPool,
        before: &mut Option<Signature>,
        log_bunches: &mut VecDeque<LogsBunch>,
        signature_with_meta: RpcConfirmedTransactionStatusWithSignature,
        commitment: CommitmentConfig,
        need_check: bool,
    ) -> Result<(), ()> {
        let signature = &Signature::from_str(&signature_with_meta.signature)
            .map_err(|err| error!("Failed to parse signature: {}", err))?;
        before.replace(*signature);
        let transaction = rpc_pool
            .with_read_rpc_loop(
                |rpc| async move {
                    rpc.get_transaction_with_config(
                        signature,
                        RpcTransactionConfig {
                            encoding: Some(UiTransactionEncoding::Json),
                            commitment: Some(commitment),
                            max_supported_transaction_version: Some(0),
                        },
                    )
                    .await
                },
                commitment,
            )
            .await;

        let logs = transaction
            .transaction
            .meta
            .filter(|meta| meta.err.is_none())
            .map(|meta| <Option<Vec<String>>>::from(meta.log_messages))
            .ok_or(())?
            .ok_or(())?;

        if logs.is_empty() {
            return Ok(());
        }

        log_bunches.push_front(LogsBunch {
            need_check,
            tx_signature: signature_with_meta.signature,
            slot: transaction.slot,
            logs,
        });
        Ok(())
    }

    async fn get_signatures_chunk(
        program_id: &Pubkey,
        solana_config: &SolanaClientConfig,
        rpc_pool: &RpcPool,
        until: Option<Signature>,
        before: Option<Signature>,
    ) -> Result<Vec<RpcConfirmedTransactionStatusWithSignature>, EventListenerError> {
        let signatures_backward = rpc_pool
            .with_read_rpc_loop(
                |rpc| async move {
                    let args = GetConfirmedSignaturesForAddress2Config {
                        before,
                        until,
                        limit: None,
                        commitment: Some(solana_config.commitment),
                    };
                    rpc.get_signatures_for_address_with_config(program_id, args).await
                },
                solana_config.commitment,
            )
            .await;
        Ok(signatures_backward)
    }
}
