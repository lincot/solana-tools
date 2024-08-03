use std::time::Instant;

use solana_sdk::{signature::Keypair, signer::Signer};
use solana_transactor::{ix_compiler::InstructionBundle, RpcEntry, RpcPool, SolanaTransactor};
use spl_token::instruction::transfer;

#[tokio::test]
async fn test_send() {
    env_logger::Builder::new().filter_level(log::LevelFilter::Info).init();

    let read_rpcs = [RpcEntry {
        url: "https://api.mainnet-beta.solana.com".to_owned(),
        ratelimit: 1,
    }];
    let write_rpcs = [RpcEntry {
        url: "https://api.mainnet-beta.solana.com".to_owned(),
        ratelimit: 1,
    }];
    let pool = RpcPool::new(&read_rpcs, &write_rpcs).unwrap();
    let transactor = SolanaTransactor::start(pool).await.expect("Failed to init transactor");
    let k1 = read_keypair("k1.json");
    println!("K1: {}", k1.pubkey());
    let k2 = read_keypair("k2.json");
    println!("K2: {}", k2.pubkey());
    let mint = read_keypair("mint.json");
    println!("Mint: {}", mint.pubkey());

    let address1 =
        spl_associated_token_account::get_associated_token_address(&k1.pubkey(), &mint.pubkey());
    let address2 =
        spl_associated_token_account::get_associated_token_address(&k2.pubkey(), &mint.pubkey());
    let ix_transfer = transfer(&spl_token::ID, &address1, &address2, &k1.pubkey(), &[], 1).unwrap();
    let start = Instant::now();
    let transfers =
        std::iter::repeat(ix_transfer).take(100).map(|ix| InstructionBundle::new(ix, 100000));
    /*transactor
    .run_ix_stream(
        futures::stream::iter(transfers),
        &[&k1],
        k1.pubkey(),
        1,
        Some(10000),
    )
    .await
    .unwrap();*/
    let transfers: Vec<_> = transfers.collect();
    transactor
        .send_all_instructions::<&str>(
            None,
            &transfers,
            &[&k1],
            k1.pubkey(),
            100,
            &[],
            Some(10000),
            true,
        )
        .await
        .unwrap();
    println!("Elapsed: {:.2}", (start.elapsed().as_millis() as f64) / 1000.0);
}

fn read_keypair(path: &str) -> Keypair {
    let bytes: Vec<u8> =
        serde_json::from_slice(&std::fs::read(format!("keys/{}", path)).expect("Failed to read"))
            .expect("Failed to parse");
    Keypair::from_bytes(&bytes).expect("Failed to init")
}
