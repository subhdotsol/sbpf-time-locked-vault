#[cfg(test)]
mod tests {
    use mollusk_svm::{result::Check, Mollusk};
    use solana_instruction::{AccountMeta, Instruction};
    use solana_pubkey::Pubkey;
    use solana_sdk::account::Account;

    //  vault data layout (must match .s offsets)
    const VAULT_OWNER: usize = 0x00;
    const VAULT_UNLOCK: usize = 0x20;
    const VAULT_WITHDRAWN: usize = 0x28;
    const VAULT_DATA_SIZE: usize = 0x29;

    // sysvar IDs
    // Clock sysvar has a well-known fixed address on all clusters
    const CLOCK_ID: Pubkey = solana_pubkey::pubkey!("SysvarC1ock11111111111111111111111111111111");
    const SYSVAR_OWNER: Pubkey =
        solana_pubkey::pubkey!("Sysvar1111111111111111111111111111111111111");

    //  helpers

    fn make_vault_data(owner: &Pubkey, unlock_slot: u64, withdrawn: u8) -> Vec<u8> {
        let mut data = vec![0u8; VAULT_DATA_SIZE];
        data[VAULT_OWNER..VAULT_OWNER + 32].copy_from_slice(&owner.to_bytes());
        data[VAULT_UNLOCK..VAULT_UNLOCK + 8].copy_from_slice(&unlock_slot.to_le_bytes());
        data[VAULT_WITHDRAWN] = withdrawn;
        data
    }

    // Clock sysvar data: slot(u64), epoch_start_timestamp(i64),
    //                    epoch(u64), leader_schedule_epoch(u64), unix_timestamp(i64)
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

    // shared setup

    fn setup() -> (Pubkey, Mollusk) {
        let program_id_bytes: [u8; 32] = std::fs::read("deploy/time-locked-vault-keypair.json")
            .unwrap()[..32]
            .try_into()
            .expect("slice with incorrect length");

        let program_id = Pubkey::from(program_id_bytes);
        let mollusk = Mollusk::new(&program_id, "deploy/time-locked-vault");
        (program_id, mollusk)
    }

    // INITIALIZE TESTS

    #[test]
    fn test_initialize_success() {
        let (program_id, mollusk) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let unlock_slot: u64 = 100;

        // ix data: [0x00=IX_INIT] ++ unlock_slot as le u64
        let mut ix_data = vec![0x00u8];
        ix_data.extend_from_slice(&unlock_slot.to_le_bytes());

        let instruction = Instruction::new_with_bytes(
            program_id,
            &ix_data,
            vec![
                AccountMeta::new(vault_pda, false),    // [0] vault (writable)
                AccountMeta::new_readonly(user, true), // [1] signer
            ],
        );

        let vault_account = Account {
            lamports: mollusk.sysvars.rent.minimum_balance(VAULT_DATA_SIZE),
            data: vec![0u8; VAULT_DATA_SIZE],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        };
        let user_account = Account::new(1_000_000_000, 0, &Pubkey::default());

        let result = mollusk.process_and_validate_instruction(
            &instruction,
            &[(vault_pda, vault_account), (user, user_account)],
            &[Check::success()],
        );

        let vault_after = result
            .resulting_accounts
            .iter()
            .find(|(k, _)| *k == vault_pda)
            .map(|(_, a)| a)
            .expect("vault missing from result");

        assert_eq!(
            vault_owner_bytes(&vault_after.data),
            user.to_bytes(),
            "vault owner should be the user pubkey"
        );
        assert_eq!(
            vault_unlock(&vault_after.data),
            unlock_slot,
            "unlock_slot should match ix data"
        );
        assert_eq!(
            vault_withdrawn(&vault_after.data),
            0,
            "withdrawn should be 0 after init"
        );
    }

    // WITHDRAW TESTS

    #[test]
    fn test_withdraw_success() {
        let (program_id, mut mollusk) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let unlock_slot: u64 = 50;
        let current_slot: u64 = 100;
        let vault_lamports: u64 = 500_000_000;
        let dest_lamports: u64 = 100_000_000;

        mollusk.warp_to_slot(current_slot);

        let vault_account = Account {
            lamports: vault_lamports,
            data: make_vault_data(&user, unlock_slot, 0),
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        };
        let user_account = Account::new(1_000_000_000, 0, &Pubkey::default());
        let clock_data = make_clock_data(current_slot);
        let clock_account = Account {
            lamports: mollusk.sysvars.rent.minimum_balance(clock_data.len()),
            data: clock_data,
            owner: SYSVAR_OWNER,
            executable: false,
            rent_epoch: 0,
        };
        let dest_account = Account::new(dest_lamports, 0, &Pubkey::default());

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(user, true),
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        let result = mollusk.process_and_validate_instruction(
            &instruction,
            &[
                (vault_pda, vault_account),
                (user, user_account),
                (CLOCK_ID, clock_account),
                (destination, dest_account),
            ],
            &[
                Check::success(),
                Check::account(&vault_pda).lamports(0).build(),
                Check::account(&destination)
                    .lamports(dest_lamports + vault_lamports)
                    .build(),
            ],
        );

        let vault_after = result
            .resulting_accounts
            .iter()
            .find(|(k, _)| *k == vault_pda)
            .map(|(_, a)| a)
            .unwrap();

        assert_eq!(
            vault_withdrawn(&vault_after.data),
            1,
            "withdrawn flag should be 1"
        );
    }

    #[test]
    fn test_withdraw_fails_before_unlock() {
        let (program_id, mut mollusk) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let unlock_slot: u64 = 500;
        let current_slot: u64 = 10; // before the lock

        mollusk.warp_to_slot(current_slot);

        let clock_data = make_clock_data(current_slot);
        let clock_account = Account {
            lamports: mollusk.sysvars.rent.minimum_balance(clock_data.len()),
            data: clock_data,
            owner: SYSVAR_OWNER,
            executable: false,
            rent_epoch: 0,
        };

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(user, true),
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        mollusk.process_and_validate_instruction(
            &instruction,
            &[
                (
                    vault_pda,
                    Account {
                        lamports: 500_000_000,
                        data: make_vault_data(&user, unlock_slot, 0),
                        owner: program_id,
                        executable: false,
                        rent_epoch: 0,
                    },
                ),
                (user, Account::new(1_000_000_000, 0, &Pubkey::default())),
                (CLOCK_ID, clock_account),
                (destination, Account::new(0, 0, &Pubkey::default())),
            ],
            // program does `mov64 r0, 1; exit` on failure → non-zero exit
            &[Check::instruction_err(
                solana_program::instruction::InstructionError::InvalidArgument,
            )],
        );
    }

    #[test]
    fn test_withdraw_fails_wrong_signer() {
        let (program_id, mut mollusk) = setup();

        let vault_pda = Pubkey::new_unique();
        let real_owner = Pubkey::new_unique();
        let wrong_signer = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let unlock_slot: u64 = 10;
        let current_slot: u64 = 100;

        mollusk.warp_to_slot(current_slot);

        let clock_data = make_clock_data(current_slot);
        let clock_account = Account {
            lamports: mollusk.sysvars.rent.minimum_balance(clock_data.len()),
            data: clock_data,
            owner: SYSVAR_OWNER,
            executable: false,
            rent_epoch: 0,
        };

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(wrong_signer, true), // ← wrong key
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        mollusk.process_and_validate_instruction(
            &instruction,
            &[
                (
                    vault_pda,
                    Account {
                        lamports: 500_000_000,
                        // vault was initialized with real_owner, not wrong_signer
                        data: make_vault_data(&real_owner, unlock_slot, 0),
                        owner: program_id,
                        executable: false,
                        rent_epoch: 0,
                    },
                ),
                (
                    wrong_signer,
                    Account::new(1_000_000_000, 0, &Pubkey::default()),
                ),
                (CLOCK_ID, clock_account),
                (destination, Account::new(0, 0, &Pubkey::default())),
            ],
            &[Check::instruction_err(
                solana_program::instruction::InstructionError::InvalidArgument,
            )],
        );
    }

    #[test]
    fn test_withdraw_fails_already_withdrawn() {
        let (program_id, mut mollusk) = setup();

        let vault_pda = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        mollusk.warp_to_slot(100);

        let clock_data = make_clock_data(100);
        let clock_account = Account {
            lamports: mollusk.sysvars.rent.minimum_balance(clock_data.len()),
            data: clock_data,
            owner: SYSVAR_OWNER,
            executable: false,
            rent_epoch: 0,
        };

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[0x01u8],
            vec![
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(user, true),
                AccountMeta::new_readonly(CLOCK_ID, false),
                AccountMeta::new(destination, false),
            ],
        );

        mollusk.process_and_validate_instruction(
            &instruction,
            &[
                (
                    vault_pda,
                    Account {
                        lamports: 0,
                        data: make_vault_data(&user, 10, 1), // withdrawn = 1 already
                        owner: program_id,
                        executable: false,
                        rent_epoch: 0,
                    },
                ),
                (user, Account::new(1_000_000_000, 0, &Pubkey::default())),
                (CLOCK_ID, clock_account),
                (destination, Account::new(0, 0, &Pubkey::default())),
            ],
            &[Check::instruction_err(
                solana_program::instruction::InstructionError::InvalidArgument,
            )],
        );
    }
}
