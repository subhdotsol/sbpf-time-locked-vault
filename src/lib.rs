#[cfg(test)]
mod tests {
    use litesvm::LiteSVM;
    use solana_account::Account;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::{pubkey, Pubkey};
    use solana_signer::Signer;
    use solana_transaction::Transaction;

    //  vault data layout offsets (must match .s file)
    const VAULT_OWNER: usize = 0x00;
    const VAULT_UNLOCK: usize = 0x20;
    const VAULT_WITHDRAWN: usize = 0x28;
    const VAULT_DATA_SIZE: usize = 0x29;

    // sysvar addresses
    const CLOCK_ID: Pubkey = pubkey!("SysvarC1ock11111111111111111111111111111111");
    const SYSVAR_OWNER: Pubkey = pubkey!("Sysvar1111111111111111111111111111111111111");

    //  helpers

    fn make_vault_data(owner: &Pubkey, unlock_slot: u64, withdrawn: u8) -> Vec<u8> {
        let mut data = vec![0u8; VAULT_DATA_SIZE];
        data[VAULT_OWNER..VAULT_OWNER + 32].copy_from_slice(&owner.to_bytes());
        data[VAULT_UNLOCK..VAULT_UNLOCK + 8].copy_from_slice(&unlock_slot.to_le_bytes());
        data[VAULT_WITHDRAWN] = withdrawn;
        data
    }

    // Clock sysvar layout: slot(u64) | epoch_start_timestamp(i64) |
    //                      epoch(u64) | leader_schedule_epoch(u64) | unix_timestamp(i64)
    fn make_clock_data(slot: u64) -> Vec<u8> {
        let mut data = vec![0u8; 40];
        data[0..8].copy_from_slice(&slot.to_le_bytes());
        data
    }

    fn vault_withdrawn(data: &[u8]) -> u8 {
        data[VAULT_WITHDRAWN]
    }
    fn vault_unlock(data: &[u8]) -> u64 {
        u64::from_le_bytes(data[VAULT_UNLOCK..VAULT_UNLOCK + 8].try_into().unwrap())
    }
    fn vault_owner_bytes(data: &[u8]) -> [u8; 32] {
        data[VAULT_OWNER..VAULT_OWNER + 32].try_into().unwrap()
    }

    //  shared setup
    // Loads the program ELF from the deploy directory into a fresh LiteSVM.
    // Returns (svm, program_id, fee_payer_keypair).

    fn setup() -> (LiteSVM, Pubkey, Keypair) {
        let secret_key: Vec<u8> = serde_json::from_str(
            &std::fs::read_to_string("deploy/time-locked-vault-keypair.json").unwrap(),
        )
        .unwrap();
        let program_keypair = Keypair::new_from_array(secret_key[..32].try_into().unwrap());
        let program_id = program_keypair.pubkey();

        let elf = std::fs::read("deploy/time-locked-vault.so")
            .expect("build the program first with `sbpf build`");

        let mut svm = LiteSVM::new();
        if let Err(e) = svm.add_program(program_id, &elf) {
            panic!("Failed to add program {}: {:?}", program_id, e);
        }

        // A funded payer to sign every transaction
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        (svm, program_id, payer)
    }

    // Send a single instruction as a transaction signed by `signers`.
    // `signers[0]` is always the fee payer.
    fn send(svm: &mut LiteSVM, ix: Instruction, signers: &[&Keypair]) -> Result<(), String> {
        let msg = Message::new(&[ix], Some(&signers[0].pubkey()));
        let tx = Transaction::new(signers, msg, svm.latest_blockhash());
        svm.send_transaction(tx)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }

    // INITIALIZE TESTS

    #[test]
    fn test_initialize_success() {
        let (mut svm, program_id, payer) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Keypair::new(); // the signer / future owner
        let unlock_slot: u64 = 100;

        // Vault account must already exist with enough space (program writes into it)
        svm.set_account(
            vault_pda,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(VAULT_DATA_SIZE),
                data: vec![0u8; VAULT_DATA_SIZE],
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        // User just needs lamports to sign
        svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

        // ix_data: [0x00=IX_INIT, unlock_slot as le u64]
        let mut ix_data = vec![0x00u8];
        ix_data.extend_from_slice(&unlock_slot.to_le_bytes());

        let ix = Instruction::new_with_bytes(
            program_id,
            &ix_data,
            vec![
                AccountMeta::new(vault_pda, false), // [0] vault (writable)
                AccountMeta::new_readonly(user.pubkey(), true), // [1] signer
            ],
        );

        // payer pays fees, user signs as the vault owner
        send(&mut svm, ix, &[&payer, &user]).expect("initialize should succeed");

        // assertions
        let vault = svm.get_account(&vault_pda).expect("vault account missing");

        assert_eq!(
            vault_owner_bytes(&vault.data),
            user.pubkey().to_bytes(),
            "vault owner should be the user pubkey"
        );
        assert_eq!(
            vault_unlock(&vault.data),
            unlock_slot,
            "unlock_slot should match ix data"
        );
        assert_eq!(
            vault_withdrawn(&vault.data),
            0,
            "withdrawn should be 0 after init"
        );
    }

    // WITHDRAW TESTS

    // Helper: set up a clock sysvar account at a given slot.
    fn set_clock(svm: &mut LiteSVM, slot: u64) {
        let data = make_clock_data(slot);
        svm.set_account(
            CLOCK_ID,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(data.len()),
                data,
                owner: SYSVAR_OWNER,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }

    #[test]
    fn test_withdraw_success() {
        let (mut svm, program_id, payer) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Keypair::new();
        let destination = Pubkey::new_unique();
        let unlock_slot: u64 = 50;
        let current_slot: u64 = 100; // past the lock
        let vault_lamports: u64 = 500_000_000;
        let dest_lamports: u64 = 100_000_000;

        // Set clock to current_slot
        set_clock(&mut svm, current_slot);

        // Vault: initialized, owned by user, not yet withdrawn
        svm.set_account(
            vault_pda,
            Account {
                lamports: vault_lamports,
                data: make_vault_data(&user.pubkey(), unlock_slot, 0),
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        // Destination
        svm.set_account(
            destination,
            Account {
                lamports: dest_lamports,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

        let ix = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),             // [0] vault
                AccountMeta::new_readonly(user.pubkey(), true), // [1] signer
                AccountMeta::new_readonly(CLOCK_ID, false),     // [2] clock sysvar
                AccountMeta::new(destination, false),           // [3] destination
            ],
        );

        send(&mut svm, ix, &[&payer, &user]).expect("withdraw should succeed");

        // assertions
        let vault = svm.get_account(&vault_pda).expect("vault missing");
        assert_eq!(vault.lamports, 0, "vault should be drained");
        assert_eq!(
            vault_withdrawn(&vault.data),
            1,
            "withdrawn flag should be 1"
        );

        let dest = svm.get_account(&destination).expect("destination missing");
        assert_eq!(
            dest.lamports,
            dest_lamports + vault_lamports,
            "destination should have received vault lamports"
        );
    }

    #[test]
    fn test_withdraw_fails_before_unlock() {
        let (mut svm, program_id, payer) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Keypair::new();
        let destination = Pubkey::new_unique();

        // current slot is BEFORE the unlock slot
        set_clock(&mut svm, 10);

        svm.set_account(
            vault_pda,
            Account {
                lamports: 500_000_000,
                data: make_vault_data(&user.pubkey(), 500, 0), // unlock = slot 500
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

        let ix = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(user.pubkey(), true),
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        let result = send(&mut svm, ix, &[&payer, &user]);
        assert!(result.is_err(), "withdraw before unlock should fail");
    }

    #[test]
    fn test_withdraw_fails_wrong_signer() {
        let (mut svm, program_id, payer) = setup();

        let vault_pda = Pubkey::new_unique();
        let real_owner = Keypair::new();
        let wrong_signer = Keypair::new(); // different keypair
        let destination = Pubkey::new_unique();

        set_clock(&mut svm, 100); // past unlock

        // vault is owned by real_owner
        svm.set_account(
            vault_pda,
            Account {
                lamports: 500_000_000,
                data: make_vault_data(&real_owner.pubkey(), 10, 0),
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        svm.airdrop(&wrong_signer.pubkey(), 1_000_000_000).unwrap();

        let ix = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(wrong_signer.pubkey(), true), // ← wrong key
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        let result = send(&mut svm, ix, &[&payer, &wrong_signer]);
        assert!(result.is_err(), "wrong signer should be rejected");
    }

    #[test]
    fn test_withdraw_fails_already_withdrawn() {
        let (mut svm, program_id, payer) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Keypair::new();
        let destination = Pubkey::new_unique();

        set_clock(&mut svm, 100);

        // withdrawn flag already = 1
        svm.set_account(
            vault_pda,
            Account {
                lamports: 0,
                data: make_vault_data(&user.pubkey(), 10, 1),
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();

        let ix = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(user.pubkey(), true),
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        let result = send(&mut svm, ix, &[&payer, &user]);
        assert!(result.is_err(), "double-withdraw should fail");
    }
}
