use anchor_lang::InstructionData;
use gmsol_programs::anchor_lang::ToAccountMetas;
use gmsol_solana_utils::{Program, ProgramExt};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use typed_builder::TypedBuilder;

use crate::{pda, serde::StringPubkey};

use super::callback::{Callback, CallbackParams};

/// Nonce Bytes.
pub type NonceBytes = StringPubkey;

/// A store program.
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(from_wasm_abi, into_wasm_abi))]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, TypedBuilder)]
pub struct StoreProgram {
    /// Program ID.
    #[builder(setter(into))]
    pub id: StringPubkey,
    /// Store address.
    #[builder(setter(into))]
    pub store: StringPubkey,
}

impl anchor_lang::Id for StoreProgram {
    fn id() -> Pubkey {
        gmsol_programs::gmsol_store::ID
    }
}

impl Default for StoreProgram {
    fn default() -> Self {
        use gmsol_programs::gmsol_store::ID;
        Self {
            id: ID.into(),
            store: pda::find_store_address("", &ID).0.into(),
        }
    }
}

impl Program for StoreProgram {
    fn id(&self) -> &Pubkey {
        &self.id
    }
}

impl StoreProgram {
    /// Create an instruction builder.
    #[deprecated(
        since = "0.8.0",
        note = "Use `ProgramExt::anchor_instruction` instead."
    )]
    #[allow(deprecated)]
    pub fn instruction(&self, args: impl InstructionData) -> InstructionBuilder {
        InstructionBuilder(self.anchor_instruction(args))
    }

    pub(crate) fn get_callback_params(&self, callback: Option<&Callback>) -> CallbackParams {
        match callback {
            Some(callback) => CallbackParams {
                callback_version: Some(callback.version),
                callback_authority: Some(self.find_callback_authority_address()),
                callback_program: Some(callback.program.0),
                callback_shared_data_account: Some(callback.shared_data.0),
                callback_partitioned_data_account: Some(callback.partitioned_data.0),
            },
            None => CallbackParams::default(),
        }
    }

    /// Find the event authority address.
    pub fn find_event_authority_address(&self) -> Pubkey {
        pda::find_event_authority_address(&self.id).0
    }

    /// Find the store wallet address.
    pub fn find_store_wallet_address(&self) -> Pubkey {
        pda::find_store_wallet_address(&self.store, &self.id).0
    }

    /// Find order address.
    pub fn find_order_address(&self, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        pda::find_order_address(&self.store, owner, &nonce.0.to_bytes(), &self.id).0
    }

    /// Find deposit address.
    pub fn find_deposit_address(&self, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        pda::find_deposit_address(&self.store, owner, &nonce.0.to_bytes(), &self.id).0
    }

    /// Find withdrawal address.
    pub fn find_withdrawal_address(&self, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        pda::find_withdrawal_address(&self.store, owner, &nonce.0.to_bytes(), &self.id).0
    }

    /// Find GLV deposit address.
    pub fn find_glv_deposit_address(&self, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        pda::find_glv_deposit_address(&self.store, owner, &nonce.0.to_bytes(), &self.id).0
    }

    /// Find GLV withdrawal address.
    pub fn find_glv_withdrawal_address(&self, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        pda::find_glv_withdrawal_address(&self.store, owner, &nonce.0.to_bytes(), &self.id).0
    }

    /// Find market address.
    pub fn find_market_address(&self, market_token: &Pubkey) -> Pubkey {
        pda::find_market_address(&self.store, market_token, &self.id).0
    }

    /// Find user address.
    pub fn find_user_address(&self, owner: &Pubkey) -> Pubkey {
        pda::find_user_address(&self.store, owner, &self.id).0
    }

    /// Find position address.
    pub fn find_position_address(
        &self,
        owner: &Pubkey,
        market_token: &Pubkey,
        collateral_token: &Pubkey,
        is_long: bool,
    ) -> Pubkey {
        pda::find_position_address(
            &self.store,
            owner,
            market_token,
            collateral_token,
            is_long,
            &self.id,
        )
        .0
    }

    /// Find the PDA for callback authority.
    pub fn find_callback_authority_address(&self) -> Pubkey {
        crate::pda::find_callback_authority(&self.id).0
    }

    /// Find the PDA for GLV account.
    pub fn find_glv_address(&self, glv_token: &Pubkey) -> Pubkey {
        crate::pda::find_glv_address(glv_token, &self.id).0
    }
}

/// Builder for [`StoreProgram`] instructions.
pub trait StoreProgramIxBuilder {
    /// Returns the [`StoreProgram`].
    fn store_program(&self) -> &StoreProgram;
}

/// Builder for Store Program Instruction.
#[deprecated(
    since = "0.8.0",
    note = "Use `gmsol_sdk::solana_utils::InstructionBuilder` instead."
)]
pub struct InstructionBuilder<'a>(gmsol_solana_utils::InstructionBuilder<'a, StoreProgram>);

#[allow(deprecated)]
impl InstructionBuilder<'_> {
    /// Append accounts.
    pub fn accounts(self, accounts: impl ToAccountMetas, convert_optional: bool) -> Self {
        Self(self.0.anchor_accounts(accounts, convert_optional))
    }

    /// Build.
    pub fn build(self) -> Instruction {
        self.0.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_store_program() {
        let program = StoreProgram::default();
        assert_eq!(
            program.store.to_string(),
            "CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN"
        );
    }

    #[cfg(serde)]
    #[test]
    fn serde() {
        let program = StoreProgram::default();
        assert_eq!(
            serde_json::to_string(&program).unwrap(),
            r#"{"id":"Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo","store":"CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN"}"#
        );
    }
}
