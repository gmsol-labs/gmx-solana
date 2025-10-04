use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_solana_utils::client_traits::FromRpcClientWith;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use typed_builder::TypedBuilder;

use crate::{
    builders::{market::MarketTokenIxBuilder, store_program::StoreProgramIxBuilder},
    serde::StringPubkey,
};

use super::NonceBytes;

pub(crate) fn generate_nonce() -> NonceBytes {
    use rand::{distributions::Standard, Rng};

    let pubkey = rand::thread_rng()
        .sample_iter(Standard)
        .take(32)
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap();
    StringPubkey(pubkey)
}

pub(crate) fn prepare_ata(
    payer: &Pubkey,
    owner: &Pubkey,
    token: Option<&Pubkey>,
    token_program_id: &Pubkey,
) -> Option<(Pubkey, Instruction)> {
    use anchor_spl::associated_token::spl_associated_token_account::instruction;

    let token = token?;

    let ata = get_associated_token_address_with_program_id(owner, token, token_program_id);

    let prepare = instruction::create_associated_token_account_idempotent(
        payer,
        owner,
        token,
        token_program_id,
    );

    Some((ata, prepare))
}

pub(crate) fn get_ata_or_owner(
    owner: &Pubkey,
    mint: &Pubkey,
    should_unwrap_native_token: bool,
) -> Pubkey {
    get_ata_or_owner_with_program_id(
        owner,
        mint,
        should_unwrap_native_token,
        &anchor_spl::token::ID,
    )
}

pub(crate) fn get_ata_or_owner_with_program_id(
    owner: &Pubkey,
    mint: &Pubkey,
    should_unwrap_native_token: bool,
    token_program_id: &Pubkey,
) -> Pubkey {
    use anchor_spl::{
        associated_token::get_associated_token_address_with_program_id,
        token::spl_token::native_mint,
    };

    if should_unwrap_native_token && *mint == native_mint::ID {
        *owner
    } else {
        get_associated_token_address_with_program_id(owner, mint, token_program_id)
    }
}

/// Hint for pool tokens.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct PoolTokenHint {
    /// Long token.
    #[builder(setter(into))]
    pub long_token: StringPubkey,
    /// Short token.
    #[builder(setter(into))]
    pub short_token: StringPubkey,
}

impl PoolTokenHint {
    /// Returns whether the given token is long token or short token.
    /// # Errors
    /// - Returns Error if the given `collateral` is not one of the specified long or short tokens.
    pub fn is_collateral_long(&self, collateral: &Pubkey) -> Result<bool, crate::SolanaUtilsError> {
        if *collateral == *self.long_token {
            Ok(true)
        } else if *collateral == *self.short_token {
            Ok(false)
        } else {
            Err(crate::SolanaUtilsError::custom(
                "invalid hint: `collateral` is not one of the specified long or short tokens",
            ))
        }
    }
}

impl<T> FromRpcClientWith<T> for PoolTokenHint
where
    T: StoreProgramIxBuilder + MarketTokenIxBuilder,
{
    async fn from_rpc_client_with<'a>(
        builder: &'a T,
        client: &'a impl gmsol_solana_utils::client_traits::RpcClient,
    ) -> gmsol_solana_utils::Result<Self> {
        use crate::{programs::gmsol_store::accounts::Market, utils::zero_copy::ZeroCopy};
        use gmsol_solana_utils::client_traits::RpcClientExt;

        let market_address = builder
            .store_program()
            .find_market_address(builder.market_token());
        let market = client
            .get_anchor_account::<ZeroCopy<Market>>(&market_address, Default::default())
            .await?
            .0;

        Ok(Self {
            long_token: market.meta.long_token_mint.into(),
            short_token: market.meta.short_token_mint.into(),
        })
    }
}
