use anchor_lang::{
    solana_program::{instruction::Instruction, message::Message, pubkey::Pubkey},
    InstructionData, ToAccountMetas,
};

#[test]
fn complete_gt_exchange_owner_is_writable_with_a_separate_fee_payer() {
    let owner = Pubkey::new_unique();
    let sponsor = Pubkey::new_unique();
    let accounts = gmsol_treasury::accounts::CompleteGtExchange {
        owner,
        store: Pubkey::new_unique(),
        config: Pubkey::new_unique(),
        treasury_vault_config: Pubkey::new_unique(),
        gt_exchange_vault: Pubkey::new_unique(),
        gt_bank: Pubkey::new_unique(),
        exchange: Pubkey::new_unique(),
        store_program: gmsol_store::ID,
        token_program: anchor_spl::token::ID,
        token_2022_program: anchor_spl::token_2022::ID,
    };
    let instruction = Instruction {
        program_id: gmsol_treasury::ID,
        accounts: accounts.to_account_metas(None),
        data: gmsol_treasury::instruction::CompleteGtExchange {}.data(),
    };
    let close_accounts = gmsol_store::accounts::CloseGtExchange {
        authority: accounts.config,
        store: accounts.store,
        owner,
        vault: accounts.gt_exchange_vault,
        exchange: accounts.exchange,
    }
    .to_account_metas(None);
    let close_owner = close_accounts
        .iter()
        .find(|meta| meta.pubkey == owner)
        .unwrap();
    assert!(
        close_owner.is_writable,
        "closing the exchange refunds SOL to its owner"
    );

    // The owner-as-payer case masks missing write permissions. Compile the same
    // instruction with a separate sponsor, without any other instruction that
    // could promote the owner to writable.
    for payer in [owner, sponsor] {
        let message = Message::new(std::slice::from_ref(&instruction), Some(&payer));
        let owner_index = message
            .account_keys
            .iter()
            .position(|key| *key == owner)
            .unwrap();
        assert!(message.is_signer(owner_index));
        assert!(
            message.is_maybe_writable(owner_index, None),
            "Treasury must provide Store's writable owner even when the owner is not the fee payer"
        );
    }
}

#[test]
fn complete_gt_exchange_v2_has_unsigned_owner_and_separate_writable_receiver() {
    let owner = Pubkey::new_unique();
    let receiver = Pubkey::new_unique();
    let sponsor = Pubkey::new_unique();
    let bank = Pubkey::new_unique();
    let exchange = Pubkey::new_unique();
    let vault = Pubkey::new_unique();
    let accounts = gmsol_treasury::accounts::CompleteGtExchangeV2 {
        owner,
        rent_receiver: receiver,
        store: Pubkey::new_unique(),
        config: Pubkey::new_unique(),
        treasury_vault_config: Pubkey::new_unique(),
        gt_exchange_vault: vault,
        gt_bank: bank,
        exchange,
        store_program: gmsol_store::ID,
        token_program: anchor_spl::token::ID,
        token_2022_program: anchor_spl::token_2022::ID,
    };
    let metas = accounts.to_account_metas(None);
    assert_eq!(metas.len(), 11);
    assert_eq!(metas[0].pubkey, owner);
    assert!(!metas[0].is_signer && !metas[0].is_writable);
    assert_eq!(metas[1].pubkey, receiver);
    assert!(!metas[1].is_signer && metas[1].is_writable);
    assert_eq!(metas[5].pubkey, vault);
    assert_eq!(metas[6].pubkey, bank);
    assert!(metas[6].is_writable);
    assert_eq!(metas[7].pubkey, exchange);
    assert!(metas[7].is_writable);
    assert_eq!(metas[9].pubkey, anchor_spl::token::ID);
    assert_eq!(metas[10].pubkey, anchor_spl::token_2022::ID);
    let instruction = Instruction {
        program_id: gmsol_treasury::ID,
        accounts: metas,
        data: gmsol_treasury::instruction::CompleteGtExchangeV2 {}.data(),
    };
    let message = Message::new(&[instruction], Some(&sponsor));
    let owner_index = message
        .account_keys
        .iter()
        .position(|key| *key == owner)
        .unwrap();
    let receiver_index = message
        .account_keys
        .iter()
        .position(|key| *key == receiver)
        .unwrap();
    assert!(!message.is_signer(owner_index));
    assert!(!message.is_maybe_writable(owner_index, None));
    assert!(message.is_maybe_writable(receiver_index, None));
}

#[test]
fn test_only_confirmation_is_not_in_public_treasury_idl() {
    let idl = include_str!("../../../crates/programs/idls/gmsol_treasury.json");
    assert!(!idl.contains("confirm_gt_bank_for_test"));
}

#[cfg(not(feature = "test-only"))]
#[test]
fn normal_build_rejects_test_only_confirmation_discriminator() {
    let data = anchor_lang::solana_program::hash::hash(b"global:confirm_gt_bank_for_test")
        .to_bytes()[..8]
        .to_vec();
    let result = gmsol_treasury::entry(&gmsol_treasury::ID, &[], &data);
    assert_eq!(
        result,
        Err(anchor_lang::solana_program::program_error::ProgramError::Custom(101))
    );
}
