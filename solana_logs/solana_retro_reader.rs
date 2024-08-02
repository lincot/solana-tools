use log::{debug, error, warn};
use mongodb::{
    bson::{doc, Bson, Document},
    options::{ClientOptions, Credential, FindOneOptions, ServerApi, ServerApiVersion},
    Client,
};
use solana_client::{
    rpc_client::GetConfirmedSignaturesForAddress2Config, rpc_config::RpcTransactionConfig,
    rpc_response::RpcConfirmedTransactionStatusWithSignature,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::UiTransactionEncoding;
use solana_transactor::RpcPool;
use std::time::Duration;
use std::{collections::VecDeque, str::FromStr};
use tokio::sync::mpsc::UnboundedSender;

use transmitter_common::mongodb::{mdb_solana_chain_id, MongodbConfig, MDB_LAST_BLOCK_COLLECTION};

use crate::common::{
    config::{SolanaClientConfig, SolanaListenerConfig},
    solana_logs::{solana_event_listener::LogsBunch, EventListenerError},
};

pub(super) struct SolanaRetroReader {
    mongodb_config: MongodbConfig,
    logs_sender: UnboundedSender<LogsBunch>,
}

impl SolanaRetroReader {
    pub(super) fn new(
        mongodb_config: MongodbConfig,
        logs_sender: UnboundedSender<LogsBunch>,
    ) -> SolanaRetroReader {
        SolanaRetroReader {
            mongodb_config,
            logs_sender,
        }
    }

    pub(super) async fn read_events_backward(
        &self,
        solana_config: &SolanaListenerConfig,
        mongodb_config: &MongodbConfig,
    ) -> Result<(), EventListenerError> {
        let Some(tx_read_from) = self.get_tx_read_from(solana_config, mongodb_config).await else {
            debug!("No tx_read_from found, skip retrospective reading");
            return Ok(());
        };

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
                    &photon::ID,
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

    async fn get_tx_read_from(
        &self,
        solana_config: &SolanaListenerConfig,
        mongodb_config: &MongodbConfig,
    ) -> Option<String> {
        if let Some(ref tx_read_from) = solana_config.tx_read_from {
            if !tx_read_from.is_empty() {
                return Some(tx_read_from.clone());
            }
        }
        if let Ok(result @ Some(_)) = self.get_last_processed_block(mongodb_config).await {
            return result;
        }
        None
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

    async fn get_last_processed_block(
        &self,
        mongodb_config: &MongodbConfig,
    ) -> Result<Option<String>, EventListenerError> {
        let mut client_options =
            ClientOptions::parse_async(&mongodb_config.uri).await.map_err(|err| {
                error!("Failed to parse mongodb uri: {}", err);
                EventListenerError::from(err)
            })?;
        let server_api = ServerApi::builder().version(ServerApiVersion::V1).build();
        client_options.server_api = Some(server_api);
        client_options.credential = Some(
            Credential::builder()
                .username(mongodb_config.user.clone())
                .password(mongodb_config.password.clone())
                .build(),
        );
        let client = Client::with_options(client_options).map_err(|err| {
            error!("Failed to build mondodb client: {}", err);
            EventListenerError::from(err)
        })?;
        let db = client.database(&mongodb_config.db);
        let collection = db.collection::<Document>(MDB_LAST_BLOCK_COLLECTION);

        let last_block: &str = &self.mongodb_config.key;
        let chain_id = mdb_solana_chain_id();
        let doc = collection
            .find_one(doc! { "direction": "from", "chain": chain_id }, FindOneOptions::default())
            .await
            .map_err(|err| {
                error!("Failed to request {}: {}", last_block, err);
                EventListenerError::from(err)
            })?;
        let Some(doc) = doc else {
            warn!("{}: not found", last_block);
            return Ok(None);
        };
        let Some(Bson::String(tx_signature)) = doc.get(last_block).cloned() else {
            warn!("Failed to get {} from document", last_block);
            return Ok(None);
        };
        debug!("tx_signature has been read from mongodb: {}", tx_signature);
        Ok(Some(tx_signature))
    }
}
