#![allow(clippy::never_loop)]
use std::collections::HashMap;

use clap::Parser;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    message::VersionedMessage,
    signature::Keypair,
    signer::{EncodableKey, Signer},
    transaction::{Transaction, VersionedTransaction},
};
use solana_transactor::{
    alt_manager, ix_compiler::InstructionBundle, MessageBundle, RpcEntry, RpcPool, SolanaTransactor,
};

#[derive(Parser, Debug)]
pub enum Subcommand {
    Single {
        /// Tx data in hex
        data: String,
    },
    Batch {
        /// Additional signers keypairs paths comma separated
        #[arg(long, default_value = "")]
        signers: String,
        /// Tx data in hex comma separated
        data: String,
    },
}

#[derive(Parser, Debug)]
struct Args {
    /// Use versioned tx
    #[arg(short, long)]
    use_v0: bool,

    /// Primary signer path
    #[arg(long, short)]
    primary_signer: Option<String>,

    #[clap(subcommand)]
    cmd: Subcommand,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::new().filter_level(log::LevelFilter::Info).init();
    let args = Args::parse();
    let read_rpcs = [
        RpcEntry {
            url: "https://small-quaint-river.solana-mainnet.quiknode.pro/8dee9265e6e19f6bef9363466dd80c7ed971bfd8/".to_owned(),
            ratelimit: 3,
        },
        RpcEntry {
            url: "https://thrumming-cosmological-bridge.solana-mainnet.quiknode.pro/977a5d82f7a9684645762d4cf901752c447d1670/".to_owned(),
            ratelimit: 3,
        },
        RpcEntry {
            url: "https://wandering-distinguished-energy.solana-mainnet.quiknode.pro/72dff9a93a6f2b2c770719d26e67ba7cb320c16f/".to_owned(),
            ratelimit: 3,
        },
    ];
    let write_rpcs = [
        RpcEntry {
            url: "https://wandering-distinguished-energy.solana-mainnet.quiknode.pro/72dff9a93a6f2b2c770719d26e67ba7cb320c16f/".to_owned(),
            ratelimit: 3,
        },
        RpcEntry {
            url: "https://small-quaint-river.solana-mainnet.quiknode.pro/8dee9265e6e19f6bef9363466dd80c7ed971bfd8/".to_owned(),
            ratelimit: 3,
        },
        RpcEntry {
            url: "https://thrumming-cosmological-bridge.solana-mainnet.quiknode.pro/977a5d82f7a9684645762d4cf901752c447d1670/".to_owned(),
            ratelimit: 3,
        },
    ];
    let pool = RpcPool::new(&read_rpcs, &write_rpcs).unwrap();
    let transactor = SolanaTransactor::start(pool).await.expect("Failed to init transactor");
    let k = if let Some(s) = args.primary_signer {
        Keypair::read_from_file(s).expect("Failed to read primary signer")
    } else {
        Keypair::read_from_file("/home/mint/Workspace/ENT/solana/photon/keys-mainnet/owner.json")
            .expect("Failed to read keypair")
    };

    match args.cmd {
        Subcommand::Single { data } => {
            let raw = hex::decode(data).expect("Invalid hex");
            let tx = if args.use_v0 {
                let tx: VersionedTransaction =
                    bincode::deserialize_from(&mut std::io::Cursor::new(raw))
                        .expect("Invalid v0 tx");
                tx
            } else {
                let tx: Transaction =
                    bincode::deserialize_from(&mut std::io::Cursor::new(raw)).expect("Invalid tx");
                VersionedTransaction::from(tx)
            };
            transactor
                .send::<&str>(None, &[MessageBundle::new(&tx.message, &[&k], k.pubkey())], true)
                .await
                .unwrap();
            transactor.await_all_tx().await;
        }
        Subcommand::Batch { signers, data } => {
            let signers: Vec<_> = signers
                .split(',')
                .map(|x| Keypair::read_from_file(x).expect("Failed to read keypair"))
                .collect();
            let signers_addrs: Vec<_> = signers.iter().map(|x| x.pubkey()).collect();
            println!("Additional signers: {:?}", signers_addrs);
            let iter =
                data.split(',').map(|data| hex::decode(data).expect("Invalid hex")).map(|raw| {
                    if args.use_v0 {
                        let tx: VersionedTransaction =
                            bincode::deserialize_from(&mut std::io::Cursor::new(raw))
                                .expect("Invalid v0 tx");
                        tx
                    } else {
                        let tx: Transaction =
                            bincode::deserialize_from(&mut std::io::Cursor::new(raw))
                                .expect("Invalid tx");
                        VersionedTransaction::from(tx)
                    }
                });
            let mut instructions = Vec::new();
            let mut alts: HashMap<
                solana_sdk::pubkey::Pubkey,
                solana_sdk::address_lookup_table_account::AddressLookupTableAccount,
            > = HashMap::new();
            for tx in iter {
                println!("Signatures: {:?}", tx.signatures);
                let msg = match tx.message.clone() {
                    VersionedMessage::Legacy(_) => unimplemented!(),
                    VersionedMessage::V0(v0) => v0,
                };
                let mut writable_keys = Vec::new();
                let mut readable_keys = Vec::new();
                for alt in msg.address_table_lookups {
                    log::info!("Fetching ALT {}", alt.account_key);
                    let fetched_alt = match alts.get(&alt.account_key) {
                        Some(x) => x.clone(),
                        None => transactor
                            .rpc_pool()
                            .with_read_rpc(
                                |rpc| async { alt_manager::load_alt(rpc, alt.account_key).await },
                                CommitmentConfig::finalized(),
                            )
                            .await
                            .expect("Failed to fetch ALT"),
                    };
                    alts.insert(fetched_alt.key, fetched_alt.clone());
                    for w_i in alt.writable_indexes {
                        writable_keys.push(fetched_alt.addresses[w_i as usize]);
                    }
                    for r_i in alt.readonly_indexes {
                        readable_keys.push(fetched_alt.addresses[r_i as usize]);
                    }
                }
                let all_accounts = &[
                    tx.message.static_account_keys(),
                    &writable_keys[..],
                    &readable_keys[..],
                ]
                .concat();
                for ix in msg.instructions.clone() {
                    let program_id = *ix.program_id(all_accounts);
                    let accounts: Vec<_> = ix
                        .accounts
                        .iter()
                        .map(|x| {
                            let x = *x as usize;
                            AccountMeta {
                                pubkey: all_accounts[x],
                                is_signer: tx.message.is_signer(x),
                                is_writable: tx.message.is_maybe_writable(x),
                            }
                        })
                        .collect();
                    let new_ix = Instruction::new_with_bytes(program_id, &ix.data, accounts);
                    instructions.push(InstructionBundle::new(new_ix, 40000));
                }
            }
            let alts: Vec<_> = alts.values().cloned().collect();
            let signers: Vec<_> = signers.iter().collect();
            alt_manager::send_with_alt(
                &transactor,
                &instructions,
                &k,
                &signers,
                1,
                &alts,
                Some(10000),
            )
            .await;
            transactor.await_all_tx().await;
        }
    }
}
