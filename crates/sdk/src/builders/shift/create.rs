use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use gmsol_model::num_traits::Zero;
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::CreateShiftParams,
};
use gmsol_solana_utils::{AtomicGroup, IntoAtomicGroup, ProgramExt};
use solana_sdk::system_program;
use typed_builder::TypedBuilder;

use crate::{
    builders::{
        shift::MIN_EXECUTION_LAMPORTS_FOR_SHIFT,
        utils::{generate_nonce, prepare_ata},
        MarketTokenIxBuilder, NonceBytes, StoreProgram, StoreProgramIxBuilder,
    },
    serde::StringPubkey,
};

/// Builder for the `create_shift` instruction.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct CreateShift {
    /// Program.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub program: StoreProgram,
    /// Payer (a.k.a. owner).
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Reciever.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option, into))]
    pub receiver: Option<StringPubkey>,
    /// Nonce for the shift.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option, into))]
    pub nonce: Option<NonceBytes>,
    /// The from-market token.
    #[builder(setter(into))]
    pub from_market_token: StringPubkey,
    /// From-market token account.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(into))]
    pub from_market_token_account: Option<StringPubkey>,
    /// The to-market token.
    #[builder(setter(into))]
    pub to_market_token: StringPubkey,
    /// Execution fee paid to the keeper in lamports.
    #[cfg_attr(serde, serde(default = "default_execution_lamports"))]
    #[builder(default = MIN_EXECUTION_LAMPORTS_FOR_SHIFT)]
    pub execution_lamports: u64,
    /// From-market token amount to pay.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub from_market_token_amount: u64,
    /// Minimum to-market token amount to receive.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub min_to_market_token_amount: u64,
    /// Whether to skip the creation of to-market token ATA.
    #[cfg_attr(serde, serde(default))]
    #[builder(default)]
    pub skip_to_market_token_ata_creation: bool,
}

#[cfg(serde)]
fn default_execution_lamports() -> u64 {
    MIN_EXECUTION_LAMPORTS_FOR_SHIFT
}

impl StoreProgramIxBuilder for CreateShift {
    fn store_program(&self) -> &StoreProgram {
        &self.program
    }
}

impl MarketTokenIxBuilder for CreateShift {
    fn market_token(&self) -> &anchor_lang::prelude::Pubkey {
        &self.from_market_token
    }
}

impl IntoAtomicGroup for CreateShift {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> gmsol_solana_utils::Result<AtomicGroup> {
        if self.from_market_token_amount.is_zero() {
            return Err(gmsol_solana_utils::Error::custom(
                "invalid argument: empty shift",
            ));
        }

        let owner = self.payer.0;
        let mut insts = AtomicGroup::new(&owner);

        let receiver = self.receiver.as_deref().copied().unwrap_or(owner);
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let shift = self.program.find_shift_address(&owner, &nonce);
        let token_program_id = anchor_spl::token::ID;
        let from_market_token = self.from_market_token.0;
        let to_market_token = self.to_market_token.0;

        let from_market_token_account = self
            .from_market_token_account
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                get_associated_token_address_with_program_id(
                    &owner,
                    &from_market_token,
                    &token_program_id,
                )
            });

        let (from_market_token_escrow, prepare) =
            prepare_ata(&owner, &shift, Some(&from_market_token), &token_program_id)
                .expect("must exist");
        insts.add(prepare);

        let (to_market_token_escrow, prepare) =
            prepare_ata(&owner, &shift, Some(&to_market_token), &token_program_id)
                .expect("must exist");
        insts.add(prepare);

        let (to_market_token_ata, prepare) =
            prepare_ata(&owner, &receiver, Some(&to_market_token), &token_program_id)
                .expect("must exist");
        if !self.skip_to_market_token_ata_creation {
            insts.add(prepare);
        }

        let params = CreateShiftParams {
            execution_lamports: self.execution_lamports,
            from_market_token_amount: self.from_market_token_amount,
            min_to_market_token_amount: self.min_to_market_token_amount,
        };

        let create = self
            .program
            .anchor_instruction(args::CreateShift {
                nonce: nonce.to_bytes(),
                params,
            })
            .anchor_accounts(
                accounts::CreateShift {
                    owner,
                    receiver,
                    store: self.program.store.0,
                    from_market: self.program.find_market_address(&from_market_token),
                    to_market: self.program.find_market_address(&to_market_token),
                    shift,
                    from_market_token,
                    to_market_token,
                    from_market_token_escrow,
                    to_market_token_escrow,
                    from_market_token_source: from_market_token_account,
                    to_market_token_ata,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    associated_token_program: associated_token::ID,
                },
                false,
            )
            .build();
        insts.add(create);

        Ok(insts)
    }
}

#[cfg(test)]
mod tests {
    use gmsol_solana_utils::transaction_builder::default_before_sign;
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn create_shift() -> crate::Result<()> {
        CreateShift::builder()
            .payer(Pubkey::new_unique())
            .from_market_token(Pubkey::new_unique())
            .to_market_token(Pubkey::new_unique())
            .from_market_token_amount(1_000_000_000)
            .build()
            .into_atomic_group(&())?
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )?;
        Ok(())
    }
}
