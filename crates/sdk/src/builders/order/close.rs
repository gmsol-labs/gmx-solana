use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use gmsol_programs::{
    anchor_lang::system_program,
    gmsol_store::client::{accounts, args},
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup};
use typed_builder::TypedBuilder;

use crate::{
    builders::{utils::get_ata_or_owner, StoreProgram},
    serde::StringPubkey,
};

/// Builder for the `close_order` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CloseOrder {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Order.
    #[builder(setter(into))]
    pub order: StringPubkey,
    /// Reason.
    #[builder(setter(into), default = "cancel".to_string())]
    pub reason: String,
}

/// Hint for [`CloseOrder`].
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CloseOrderHint {
    /// Owner.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Receiver.
    #[builder(setter(into))]
    pub receiver: StringPubkey,
    /// Rent Receiver.
    #[builder(setter(into))]
    pub rent_receiver: StringPubkey,
    /// Referrer.
    #[builder(setter(into))]
    pub referrer: Option<StringPubkey>,
    /// Initial collateral token.
    #[builder(setter(into))]
    pub initial_collateral_token: Option<StringPubkey>,
    /// Final output token.
    #[builder(setter(into))]
    pub final_output_token: Option<StringPubkey>,
    /// Long token.
    #[builder(setter(into))]
    pub long_token: Option<StringPubkey>,
    /// Short token.
    #[builder(setter(into))]
    pub short_token: Option<StringPubkey>,
    /// `should_unwrap_native_token` flag.
    pub should_unwrap_native_token: bool,
}

impl IntoAtomicGroup for CloseOrder {
    type Hint = CloseOrderHint;

    fn into_atomic_group(self, hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        let token_program_id = anchor_spl::token::ID;
        let payer = self.payer.0;
        let owner = hint.owner.0;
        let receiver = hint.receiver.0;
        let user = self.program.find_user_address(&owner);
        let referrer_user = hint
            .referrer
            .as_ref()
            .map(|referrer| self.program.find_user_address(referrer));
        let order = self.order.0;
        let initial_collateral_token = hint.initial_collateral_token.as_deref().copied();
        let final_output_token = hint.final_output_token.as_deref().copied();
        let long_token = hint.long_token.as_deref().copied();
        let short_token = hint.short_token.as_deref().copied();

        let close = self
            .program
            .instruction(args::CloseOrder {
                reason: self.reason,
            })
            .accounts(
                accounts::CloseOrder {
                    executor: payer,
                    store: self.program.store.0,
                    store_wallet: self.program.find_store_wallet_address(),
                    owner,
                    receiver,
                    rent_receiver: hint.rent_receiver.0,
                    user,
                    referrer_user,
                    order,
                    initial_collateral_token,
                    final_output_token,
                    long_token,
                    short_token,
                    initial_collateral_token_escrow: initial_collateral_token.as_ref().map(
                        |token| {
                            get_associated_token_address_with_program_id(
                                &order,
                                token,
                                &token_program_id,
                            )
                        },
                    ),
                    final_output_token_escrow: final_output_token.as_ref().map(|token| {
                        get_associated_token_address_with_program_id(
                            &order,
                            token,
                            &token_program_id,
                        )
                    }),
                    long_token_escrow: long_token.as_ref().map(|token| {
                        get_associated_token_address_with_program_id(
                            &order,
                            token,
                            &token_program_id,
                        )
                    }),
                    short_token_escrow: short_token.as_ref().map(|token| {
                        get_associated_token_address_with_program_id(
                            &order,
                            token,
                            &token_program_id,
                        )
                    }),
                    initial_collateral_token_ata: initial_collateral_token.as_ref().map(|token| {
                        get_ata_or_owner(&owner, token, hint.should_unwrap_native_token)
                    }),
                    final_output_token_ata: final_output_token.as_ref().map(|token| {
                        get_ata_or_owner(&receiver, token, hint.should_unwrap_native_token)
                    }),
                    long_token_ata: long_token.as_ref().map(|token| {
                        get_ata_or_owner(&receiver, token, hint.should_unwrap_native_token)
                    }),
                    short_token_ata: short_token.as_ref().map(|token| {
                        get_ata_or_owner(&receiver, token, hint.should_unwrap_native_token)
                    }),
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    associated_token_program: associated_token::ID,
                    event_authority: self.program.find_event_authority_address(),
                    program: self.program.id.0,
                },
                true,
            )
            .build();
        Ok(AtomicGroup::with_instructions(&payer, Some(close)))
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn close_order() -> crate::Result<()> {
        use anchor_spl::token::spl_token::native_mint::ID as NATIVE_MINT;

        let order = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let owner = Pubkey::new_unique();
        let receiver = Pubkey::new_unique();
        let rent_receiver = Pubkey::new_unique();
        let referrer = Some(Pubkey::new_unique().into());
        let initial_collatearl_token = Some(NATIVE_MINT.into());
        let final_output_token = None;
        let long_token = Some(Pubkey::new_unique().into());
        let short_token = Some(Pubkey::new_unique().into());
        CloseOrder::builder()
            .payer(payer)
            .order(order)
            .build()
            .into_atomic_group(
                &CloseOrderHint::builder()
                    .owner(owner)
                    .receiver(receiver)
                    .rent_receiver(rent_receiver)
                    .referrer(referrer)
                    .initial_collateral_token(initial_collatearl_token)
                    .final_output_token(final_output_token)
                    .long_token(long_token)
                    .short_token(short_token)
                    .should_unwrap_native_token(true)
                    .build(),
            )?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
            )?;
        Ok(())
    }
}
