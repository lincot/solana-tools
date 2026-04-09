use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::{v0::Message, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
};
use std::fmt::Display;

use super::TransactorError;
use crate::{log_with_ctx, utils::max_of_option};

const MAX_CU: u32 = 1_400_000;
const MAX_MSG_LEN: usize = 1232 - 65; // assuming only one signature
/// The maximum number of accounts in a transaction that will not cause the
/// "Transaction locked too many accounts" error. May be increased to 128 when
/// [the 128 accounts feature](https://github.com/solana-labs/solana/issues/27241)
/// gets enabled on mainnet.
const MAX_ACCOUNTS: usize = 64;

#[derive(Debug, Clone)]
pub struct InstructionBundle {
    pub instruction: Instruction,
    pub compute_units: u32,
    pub heap_frame: Option<u32>,
    pub alt_accounts: Vec<AddressLookupTableAccount>,
}

impl InstructionBundle {
    pub fn new(
        instruction: Instruction,
        compute_units: u32,
        heap_frame: Option<u32>,
        alt_accounts: Vec<AddressLookupTableAccount>,
    ) -> Self {
        Self {
            instruction,
            compute_units,
            heap_frame,
            alt_accounts,
        }
    }
}

pub struct IxCompiler {
    ix_buffer: Vec<Instruction>,
    total_compute_units: u32,
    max_heap_frame: Option<u32>,
    payer: Pubkey,
    alt_accounts: Vec<AddressLookupTableAccount>,
    compute_units_price: Option<u64>,
}

impl IxCompiler {
    pub fn new(payer: Pubkey, compute_units_price: Option<u64>) -> Self {
        Self {
            ix_buffer: Vec::new(),
            total_compute_units: 0,
            max_heap_frame: None,
            payer,
            alt_accounts: Vec::new(),
            compute_units_price,
        }
    }

    pub fn set_cu_price(&mut self, compute_units_price: Option<u64>) {
        self.compute_units_price = compute_units_price;
    }

    pub fn get_ix_price_if_any(&self) -> Vec<Instruction> {
        self.compute_units_price
            .map(ComputeBudgetInstruction::set_compute_unit_price)
            .into_iter()
            .collect()
    }

    /// Try to pack buffered instructions into message, return next message to send if close to or over transaction size limit
    pub fn compile<T: Display>(
        &mut self,
        log_ctx: Option<T>,
        ix: Instruction,
        alt_accounts: &[AddressLookupTableAccount],
        compute_units: u32,
        heap_frame: Option<u32>,
    ) -> Result<Option<VersionedMessage>, TransactorError> {
        // Initial instruction validation
        let msg = Message::try_compile(
            &self.payer,
            &[
                &[get_compute_units_ix(compute_units)],
                &self.get_ix_price_if_any()[..],
                &get_heap_frame_ix(heap_frame),
                std::slice::from_ref(&ix),
            ]
            .concat(),
            alt_accounts,
            Hash::default(),
        )?;
        let msg = VersionedMessage::V0(msg);
        let msg_len = msg.serialize().len();
        let account_count = get_account_count(&msg);
        if exceeds_limits(msg_len, compute_units, account_count) {
            return Err(TransactorError::InstructionTooBig);
        }

        let total_compute_units = self.total_compute_units + compute_units;
        let max_heap_frame = max_of_option(self.max_heap_frame, heap_frame);
        let ix_buffer = [
            &[get_compute_units_ix(total_compute_units)],
            &self.get_ix_price_if_any()[..],
            &get_heap_frame_ix(max_heap_frame),
            &self.ix_buffer[..],
            std::slice::from_ref(&ix),
        ]
        .concat();
        let alt_accounts_all = [&self.alt_accounts[..], alt_accounts].concat();
        let msg =
            Message::try_compile(&self.payer, &ix_buffer, &alt_accounts_all, Hash::default())?;
        let msg = VersionedMessage::V0(msg);
        let msg_len = msg.serialize().len();
        let num_accounts = get_account_count(&msg);
        log_with_ctx!(
            debug,
            log_ctx,
            "Instructions: {}, tx len: {}, CU: {}, accounts: {}",
            self.ix_buffer.len(),
            msg_len,
            total_compute_units,
            num_accounts,
        );
        if exceeds_limits(msg_len, total_compute_units, num_accounts) {
            log_with_ctx!(debug, log_ctx, "Tx limit reached, sending previous instructions...");
            let msg = Message::try_compile(
                &self.payer,
                &[
                    &[get_compute_units_ix(self.total_compute_units)],
                    &self.get_ix_price_if_any()[..],
                    &get_heap_frame_ix(self.max_heap_frame),
                    &self.ix_buffer[..],
                ]
                .concat(),
                &self.alt_accounts,
                Hash::default(),
            )?;
            self.ix_buffer.clear();
            self.ix_buffer.push(ix);
            self.alt_accounts.clear();
            self.alt_accounts.extend_from_slice(alt_accounts);
            self.total_compute_units = compute_units;
            self.max_heap_frame = heap_frame;
            return Ok(Some(VersionedMessage::V0(msg)));
        }
        self.ix_buffer.push(ix);
        self.alt_accounts.extend_from_slice(alt_accounts);
        self.total_compute_units = total_compute_units;
        self.max_heap_frame = max_heap_frame;
        Ok(None)
    }

    pub fn flush(&mut self) -> Result<Option<VersionedMessage>, TransactorError> {
        if self.ix_buffer.is_empty() {
            return Ok(None);
        }
        let msg = Message::try_compile(
            &self.payer,
            &[
                &[get_compute_units_ix(self.total_compute_units)],
                &self.get_ix_price_if_any()[..],
                &get_heap_frame_ix(self.max_heap_frame),
                &self.ix_buffer[..],
            ]
            .concat(),
            &self.alt_accounts,
            Hash::default(),
        )?;
        self.ix_buffer.clear();
        self.alt_accounts.clear();
        self.total_compute_units = 0;
        self.max_heap_frame = None;
        Ok(Some(VersionedMessage::V0(msg)))
    }
}

/// Returns true if tx exceeds limits
fn exceeds_limits(msg_len: usize, compute_units: u32, account_count: usize) -> bool {
    msg_len > MAX_MSG_LEN || compute_units > MAX_CU || account_count > MAX_ACCOUNTS
}

fn get_compute_units_ix(compute_units: u32) -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(compute_units)
}

fn get_heap_frame_ix(heap_frame: Option<u32>) -> Vec<Instruction> {
    heap_frame.map(ComputeBudgetInstruction::request_heap_frame).into_iter().collect()
}

fn get_account_count(message: &VersionedMessage) -> usize {
    message.static_account_keys().len()
        + message.address_table_lookups().map_or(0, |lookups| {
            lookups
                .iter()
                .map(|lookup| lookup.writable_indexes.len() + lookup.readonly_indexes.len())
                .sum()
        })
}

#[cfg(test)]
mod test {
    use solana_sdk::{
        instruction::AccountMeta, signature::Keypair, signer::Signer,
        transaction::VersionedTransaction,
    };

    use super::*;

    #[test]
    fn test_ix_compile() {
        env_logger::Builder::new().filter_level(log::LevelFilter::Debug).init();
        let signer = Keypair::new();
        let program = Keypair::new();
        let accounts = [
            &vec![AccountMeta::new(Keypair::new().pubkey(), false); 9][..],
            &[AccountMeta::new(signer.pubkey(), true)],
        ]
        .concat();
        let ix = Instruction::new_with_bytes(program.pubkey(), &[1; 128], accounts);
        let mut ix_compiler = IxCompiler::new(signer.pubkey(), Some(1000));
        let mut n = 0;
        let msg = loop {
            if let Some(msg) =
                ix_compiler.compile::<&str>(None, ix.clone(), &[], 20000, None).unwrap()
            {
                break msg;
            } else {
                n += 1;
            }
        };
        let tx = VersionedTransaction::try_new(msg, &[&signer]).unwrap();
        let tx_raw: Vec<u8> = bincode::serialize(&tx).unwrap();
        println!("Packed {} ix", n);
        println!("Tx len {}", tx_raw.len());
        assert!(tx_raw.len() <= 1232);
        let flush_msg = ix_compiler.flush().unwrap().unwrap();
        let tx = VersionedTransaction::try_new(flush_msg, &[&signer]).unwrap();
        let tx_raw: Vec<u8> = bincode::serialize(&tx).unwrap();
        println!("Flush tx len {}", tx_raw.len());
        assert!(tx_raw.len() <= 1232);

        let msg =
            ix_compiler.compile::<&str>(None, ix.clone(), &[], 1200000, None).unwrap().unwrap();
        let tx = VersionedTransaction::try_new(msg, &[&signer]).unwrap();
        let tx_raw: Vec<u8> = bincode::serialize(&tx).unwrap();
        assert!(tx_raw.len() <= 1232);
    }
}
