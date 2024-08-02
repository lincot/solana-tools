use log::debug;
use std::{collections::HashSet, time::Duration};

use solana_address_lookup_table_program::{
    instruction::{
        close_lookup_table, create_lookup_table, deactivate_lookup_table, extend_lookup_table,
    },
    state::AddressLookupTable,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table_account::AddressLookupTableAccount, commitment_config::CommitmentConfig,
    instruction::AccountMeta, pubkey::Pubkey, signature::Keypair, signer::Signer, system_program,
};

use crate::{ix_compiler::InstructionBundle, SolanaTransactor, TransactorError};

pub async fn send_with_alt(
    transactor: &SolanaTransactor,
    instructions: &[InstructionBundle],
    signer: &Keypair,
    additional_signers: &[&Keypair],
    parallel_limit: usize,
    alt: &[AddressLookupTableAccount],
    compute_unit_price: Option<u64>,
) {
    let alt_addresses: Vec<_> = alt.iter().flat_map(|x| x.addresses.clone().into_iter()).collect();
    let total_addresses: Vec<_> = instructions
        .iter()
        .flat_map(|x| {
            x.instruction
                .accounts
                .clone()
                .into_iter()
                .map(|x| x.pubkey)
                .chain(std::iter::once(x.instruction.program_id))
        })
        .collect();
    let to_add: HashSet<_> =
        total_addresses.iter().cloned().filter(|x| !alt_addresses.contains(x)).collect();
    debug!("Total addresses: {}", total_addresses.len());
    debug!("To add: {}", to_add.len());
    let slot = transactor
        .rpc_pool()
        .with_read_rpc(|rpc| async move { rpc.get_slot().await }, CommitmentConfig::confirmed())
        .await
        .expect("Failed to get slot");
    let (ix, alt_address) = create_lookup_table(signer.pubkey(), signer.pubkey(), slot);
    debug!("New ALT address {}", alt_address);
    let ix = InstructionBundle::new(ix, 200000);
    transactor
        .send_all_instructions::<&str>(
            None,
            &[ix],
            &[signer],
            signer.pubkey(),
            parallel_limit,
            &[],
            compute_unit_price,
            true,
        )
        .await
        .expect("Failed to send create ALT instruction");
    debug!("ALT created");
    let to_add: Vec<_> = to_add.into_iter().collect();
    for chunk in to_add.chunks(20) {
        let mut ix = extend_lookup_table(
            alt_address,
            signer.pubkey(),
            Some(signer.pubkey()),
            chunk.to_vec(),
        );
        ix.accounts.push(AccountMeta {
            pubkey: system_program::ID,
            is_signer: false,
            is_writable: false,
        });
        let ix = InstructionBundle::new(ix, 200000);
        transactor
            .send_all_instructions::<&str>(
                None,
                &[ix],
                &[signer],
                signer.pubkey(),
                parallel_limit,
                &[],
                compute_unit_price,
                true,
            )
            .await
            .expect("Failed to send create ALT instruction");
        debug!("ALT extended by {}", chunk.len());
    }
    tokio::time::sleep(Duration::from_secs(20)).await;
    let new_alt = transactor
        .rpc_pool()
        .with_read_rpc(
            |rpc| async move { load_alt(rpc, alt_address).await },
            CommitmentConfig::confirmed(),
        )
        .await
        .expect("Failed to load new ALT");
    if let Err(e) = transactor
        .send_all_instructions::<&str>(
            None,
            instructions,
            &[&[signer], additional_signers].concat(),
            signer.pubkey(),
            parallel_limit,
            &[&[new_alt], alt].concat(),
            compute_unit_price,
            true,
        )
        .await
    {
        log::error!("Failed to send tx: {}", e);
    }
    debug!("Deactivating ALT");
    let ix = deactivate_lookup_table(alt_address, signer.pubkey());
    let ix = InstructionBundle::new(ix, 200000);
    transactor
        .send_all_instructions::<&str>(
            None,
            &[ix],
            &[signer],
            signer.pubkey(),
            parallel_limit,
            &[],
            compute_unit_price,
            true,
        )
        .await
        .expect("Failed to deactivate ALT");
    tokio::time::sleep(Duration::from_secs(240)).await;
    debug!("Clearing ALT");
    let ix = close_lookup_table(alt_address, signer.pubkey(), signer.pubkey());
    let ix = InstructionBundle::new(ix, 200000);
    transactor
        .send_all_instructions::<&str>(
            None,
            &[ix],
            &[signer],
            signer.pubkey(),
            parallel_limit,
            &[],
            compute_unit_price,
            true,
        )
        .await
        .expect("Failed to close ALT");
}

pub async fn load_alt(
    client: RpcClient,
    address: Pubkey,
) -> Result<AddressLookupTableAccount, TransactorError> {
    let raw_account = client.get_account(&address).await?;
    let address_lookup_table = AddressLookupTable::deserialize(&raw_account.data)?;
    Ok(AddressLookupTableAccount {
        key: address,
        addresses: address_lookup_table.addresses.to_vec(),
    })
}
