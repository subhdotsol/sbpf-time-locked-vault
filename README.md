# Time Locked Vault

A custom engineered sBPF assembly implementation of a temporal asset locking mechanism for the Solana Virtual Machine. This project prioritizes execution speed and byte level precision by interacting directly with the program architecture at its lowest level.

## What is this?

This program serves as a specialized primitive for locking lamports within a Program Derived Address (PDA). It allows a user to commit funds to a secure vault that can only be unlocked and withdrawn after a specific blockchain slot has passed. By using raw assembly rather than a high level framework, the vault eliminates the overhead of boilerplate code and complex serialization, resulting in one of the most lightweight ways to handle time locks on Solana.

## Architecture

The system is designed around two primary execution paths: Initialize and Withdraw. 

The program operates by directly manipulating the r0 and r1 registers to access the account list and instruction data. It relies on fixed memory offsets for data retrieval, which significantly reduces the compute budget consumed during instruction processing.

Internal state is managed within a 41 byte PDA layout:
*   Owner Identity: 32 bytes (Pubkey)
*   Lock Duration: 8 bytes (u64 slot)
*   State Flag: 1 byte (Withdrawal status)

Security is enforced through a series of assembly level comparisons that verify the signer against the stored owner and the current network slot against the target lock time.

## Project Structure

The repository is organized to separate the assembly logic from the testing and deployment infrastructure:

*   src/time-locked-vault/time-locked-vault.s: The core logic written in sBPF assembly.
*   src/lib.rs: The Rust integration and unit test suite.
*   deploy/: Contains the compiled program binaries and identity keypairs.
*   Cargo.toml: Defines the development environment and SVM simulation dependencies.

## Testing and Benchmarking

Validation is performed using the Mollusk SVM simulation framework, which allows for rigorous testing of assembly instructions without the need for a full local validator.

The test suite covers:
*   Success of vault initialization with correct data offsets.
*   Verification of lock enforcement before the target slot is reached.
*   Successful withdrawal and lamport transfer once the time lock has expired.

Because this program is written in raw sBPF, its compute unit usage is near the theoretical minimum for the Solana runtime. Benchmarking focuses on minimizing instruction counts during the transfer of lamports between the vault and the destination account.

## Development

To compile the assembly and run the validation suite, use the standard cargo test command:

cargo test sbf

This process assembles the source file and executes the integrated SVM tests to ensure the logic remains sound across different runtime versions.
