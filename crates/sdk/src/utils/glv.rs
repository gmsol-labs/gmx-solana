use std::collections::BTreeSet;

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

/// Split accounts for constructing GLV instruction.
pub fn split_to_accounts(
    market_tokens: impl IntoIterator<Item = Pubkey>,
    glv: &Pubkey,
    store: &Pubkey,
    store_program_id: &Pubkey,
    token_program_id: &Pubkey,
    with_vaults: bool,
) -> (Vec<AccountMeta>, usize) {
    let market_token_addresses = market_tokens.into_iter().collect::<BTreeSet<_>>();

    let markets = market_token_addresses.iter().map(|token| {
        AccountMeta::new_readonly(
            crate::pda::find_market_address(store, token, store_program_id).0,
            false,
        )
    });

    let market_tokens = market_token_addresses
        .iter()
        .map(|token| AccountMeta::new_readonly(*token, false));

    let length = market_token_addresses.len();

    let accounts = if with_vaults {
        let market_token_vaults = market_token_addresses.iter().map(|token| {
            let market_token_vault =
                get_associated_token_address_with_program_id(glv, token, token_program_id);

            AccountMeta::new(market_token_vault, false)
        });

        markets
            .chain(market_tokens)
            .chain(market_token_vaults)
            .collect::<Vec<_>>()
    } else {
        markets.chain(market_tokens).collect::<Vec<_>>()
    };

    (accounts, length)
}
