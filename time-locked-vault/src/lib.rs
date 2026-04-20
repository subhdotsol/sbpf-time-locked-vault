#[cfg(test)]
mod tests {
    use mollusk_svm::{result::Check, Mollusk};
    use solana_address::Address;
    use solana_instruction::Instruction;

    #[test]
    fn test_hello_world() {
        let program_id_keypair_bytes = std::fs::read("deploy/time-locked-vault-keypair.json").unwrap()
            [..32]
            .try_into()
            .expect("slice with incorrect length");
        let program_id = Address::new_from_array(program_id_keypair_bytes);

        let instruction = Instruction::new_with_bytes(
            program_id,
            &[],
            vec![]
        );

        let mollusk = Mollusk::new(&program_id, "deploy/time-locked-vault");

        let result = mollusk.process_and_validate_instruction(
            &instruction,
            &[],
            &[Check::success()]
        );
        assert!(!result.program_result.is_err());
    }
}