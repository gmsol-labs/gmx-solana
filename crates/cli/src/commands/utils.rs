use eyre::OptionExt;
use gmsol_sdk::{
    client::token_map::TokenMap,
    core::token_config::TokenMapAccess,
    programs::{anchor_lang::prelude::Pubkey, gmsol_store::accounts::Market},
    utils::Amount,
};

pub(crate) fn token_amount(
    amount: &Amount,
    token: Option<&Pubkey>,
    token_map: &TokenMap,
    market: &Market,
    is_long: bool,
) -> eyre::Result<u64> {
    let token = match token {
        Some(token) => token,
        None => {
            if is_long {
                &market.meta.long_token_mint
            } else {
                &market.meta.short_token_mint
            }
        }
    };
    let decimals = token_map
        .get(token)
        .ok_or_eyre("token config not found")?
        .token_decimals;
    Ok(amount.to_u64(decimals)?)
}
