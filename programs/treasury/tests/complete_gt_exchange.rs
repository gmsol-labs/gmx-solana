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
