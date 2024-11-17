<img src="doc/entangle.avif" alt="Entangle" style="width:100%;"/>

# Collection of utilities to send transactions to Solana

This project provides a set of tools and utilities to facilitate the creation, management, and execution of transactions 
on the Solana blockchain. It includes two main components:

- **solana-transactor**: For creating and managing Address Lookup Tables (ALTs), sending instructions, and handling various transaction-related operations.
- **solana_logs**: For logging transaction details, monitoring transaction status, and generating reports for transaction activities.

This repo is supposed to be included as a submodule in the main project, where the utilities are used.


## Table of Contents

- [Solana transactor](#solana-transactor)
- [Solana logs](#solana-logs)
- [Including in your project](#including-in-your-project)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)
- [License](LICENSE)

## Solana transactor

- Sending multiple instructions in parallel
- Handling transaction signing and submission
- Supporting for custom compute unit prices
- Creating and managing Address Lookup Tables (ALTs)

## Solana logs

- Log transaction details
- Monitor transaction status
- Generate reports for transaction activities

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

