use std::collections::HashSet;

use anchor_spl::token::spl_token::native_mint;
use either::Either;
use solana_sdk::pubkey::Pubkey;
use typed_builder::TypedBuilder;

use crate::{serde::StringPubkey, AtomicGroup, IntoAtomicGroup};

use super::utils::prepare_ata;

/// Prepare token accounts for the owner.
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct PrepareTokenAccounts {
    /// Payer.
    #[builder(setter(into))]
    pub payer: StringPubkey,
    /// Owner.
    #[cfg_attr(serde, serde(default))]
    #[builder(default, setter(strip_option, into))]
    pub owner: Option<StringPubkey>,
    /// Tokens.
    #[builder(setter(into))]
    pub tokens: HashSet<StringPubkey>,
    /// Token Program ID.
    #[builder(default = StringPubkey(anchor_spl::token::ID), setter(into))]
    pub token_program: StringPubkey,
}

impl IntoAtomicGroup for PrepareTokenAccounts {
    type Hint = ();

    fn into_atomic_group(self, _hint: &Self::Hint) -> Result<AtomicGroup, crate::SolanaUtilsError> {
        let payer = self.payer.0;
        let owner = self.owner.as_deref().copied().unwrap_or(payer);
        let insts = self.tokens.iter().map(|token| {
            prepare_ata(&payer, &owner, Some(token), &self.token_program)
                .unwrap()
                .1
        });
        Ok(AtomicGroup::with_instructions(&payer, insts))
    }
}

/// Wraps the native token into its corresponding associated token account
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct WrapNative {
    /// Owner.
    #[builder(setter(into))]
    pub owner: StringPubkey,
    /// Lamports.
    pub lamports: u64,
}

impl WrapNative {
    /// Native mint.
    pub const NATIVE_MINT: Pubkey = native_mint::ID;
}

/// Whether to skip the ATA preparation.
pub type SkipPreparation = bool;

impl IntoAtomicGroup for WrapNative {
    type Hint = SkipPreparation;

    fn into_atomic_group(
        self,
        skip_preparation: &Self::Hint,
    ) -> Result<AtomicGroup, crate::SolanaUtilsError> {
        use anchor_spl::token::{spl_token::instruction::sync_native, ID};
        use gmsol_programs::anchor_lang::solana_program::system_instruction::transfer;

        let owner = self.owner.0;
        let (ata, prepare) = prepare_ata(&owner, &owner, Some(&Self::NATIVE_MINT), &ID).unwrap();
        let transfer = transfer(&owner, &ata, self.lamports);
        let sync = sync_native(&ID, &ata).unwrap();

        Ok(AtomicGroup::with_instructions(
            &owner,
            if *skip_preparation {
                Either::Left([transfer, sync].into_iter())
            } else {
                Either::Right([prepare, transfer, sync].into_iter())
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use gmsol_solana_utils::transaction_builder::default_before_sign;
    use solana_sdk::pubkey::Pubkey;

    use super::*;

    #[test]
    fn prepare_token_accounts() {
        let tokens = [Pubkey::new_unique().into(), Pubkey::new_unique().into()];
        let insts = PrepareTokenAccounts::builder()
            .payer(Pubkey::new_unique())
            .tokens(tokens)
            .build()
            .into_atomic_group(&())
            .unwrap();
        assert_eq!(insts.len(), tokens.len());
        insts
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )
            .unwrap();
    }

    #[test]
    fn wrap_native() {
        WrapNative::builder()
            .owner(Pubkey::new_unique())
            .lamports(1_000_000_000)
            .build()
            .into_atomic_group(&true)
            .unwrap()
            .partially_signed_transaction_with_blockhash_and_options(
                Default::default(),
                Default::default(),
                None,
                default_before_sign,
            )
            .unwrap();
    }
}
