<img src="doc/entangle.avif" alt="Entangle" style="width:100%;"/>

# Solana Tools. Transacting and log monitoring

This project provides a set of tools and utilities to facilitate the creation, management, and execution of transactions 
on the Solana blockchain. It includes two main components:

- **solana-transactor**: For creating and managing Address Lookup Tables (ALTs), sending instructions, and handling various transaction-related operations.
- **solana_logs**: For logging transaction details, monitoring transaction status, and generating reports for transaction activities.

This repo is supposed to be included as a submodule in the main project, where the utilities are used.

## Table of Contents

- [Including in your project](#including-in-your-project)
- [Solana transactor](#solana-transactor)
- [Solana logs](#solana-logs)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)
- [License](LICENSE)

## Including in your project

To use this project, add the following to your `Cargo.toml`:

### .gitmodules

```
[submodule "solana-tools"]
	path = solana-tools
	url = https://github.com/Entangle-Protocol/solana-tools.git
	branch = main
```

### Cargo.toml

```toml
[dependencies]
solana-tools = { path = "../solana-tools" }
```

## Solana transactor

This implementation ensures reliable and flexible transaction processing in Solana by abstracting the complexities of 
RPC interactions. Thanks to RpcPool, the risk of failure due to RPC unavailability is minimized, making this approach 
particularly suitable for high-load or mission-critical applications. That also facilitates: sending multiple instructions in parallel, 
handling transaction signing and submission, supporting for custom compute unit prices, creating and managing Address Lookup Tables (ALTs).

**Example of usage**

The `extend_with_timestamp` function sends a transaction to the Solana network using SolanaTransactor, adding a new 
instruction with specified compute units and their cost. A key feature of this implementation is the use of RpcPool, 
built into SolanaTransactor.

RpcPool automatically handles issues with RPC API availability. If the primary RPC server becomes unavailable,
the pool seamlessly switches to a backup server, ensuring stable interaction with the Solana network. This approach 
mitigates potential network disruptions and enhances the reliability and performance of transaction submission.

The `send_all_instructions` method enables efficient handling of both single and grouped instructions.

```rust
use solana_tools::solana_transactor::{ix_compiler::InstructionBundle, RpcPool, SolanaTransactor};


pub(super) struct CustomProcessor {
    transactor: SolanaTransactor,
}

impl CustomProcessor {
    fn extend_with_timestamp(&self, instruction_bundles: Vec<InstructionBundle>) {
        // The main purpose of a transactor with the rpc pool inside is to provide an ability to use alternative one if the main gets un
        let ix =
            Instruction::new_with_bytes(udf_solana::id(), &update_data, accounts);
        let bundle = vec![InstructionBundle::new(ix, 400000)];
        const COMPUTE_UNIT_PRICE_LAMPORTS: u64 = 1000;
        self.transactor
            .send_all_instructions::<&str>(
                None,
                &bundle,
                &[publisher],
                publisher.pubkey(),
                1,
                &[],
                Some(COMPUTE_UNIT_PRICE_LAMPORTS),
                false,
            )
            .await
            .map_err(|err| error!("Failed to process transaction: {}", err))?;
    }
}
```

## Solana logs


The main feature provided by solana_logs is the ability to define custom events and process them.
An event processor is a trait that must be implemented for each custom event. As a result, the event processor extracts
the target event from all the logs generated by the target contract and processes it.

- Log transaction details
- Monitor transaction status
- Generate reports for transaction activities

**Example of usage**

```rust

use solana_tools::solana_logs::{EventProcessor, LogsBunch, SolanaListenerConfig};

#[event]
pub(crate) struct CustomEvent {
    pub(crate) entangle: Pubkey,
    pub(crate) the: u64,
    pub(crate) best: u64,
}

impl EventProcessor for MintedEventProcessor {
    type Event = CustomEvent;

    fn on_event(&self, protocol: String, event: Self::Event, tx_signature: &str, slot: u64, _need_check: bool) {
        self.minted_sender
            .send(event)
            .expect("Expected to be sent");
    }
}
```
