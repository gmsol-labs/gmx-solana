use gmsol_programs::anchor_lang::{InstructionData, ToAccountMetas};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use typed_builder::TypedBuilder;

use crate::{
    pda,
    utils::{optional::fix_optional_account_metas, serde::StringPubkey},
};

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

impl Default for StoreProgram {
    fn default() -> Self {
        use gmsol_programs::gmsol_store::ID;
        Self {
            id: ID.into(),
            store: pda::find_store_address("", &ID).0.into(),
        }
    }
}

impl StoreProgram {
    /// Convert to account metas.
    ///
    /// If `convert_optional` is `true`, read-only non-signer accounts with
    /// the default program ID as pubkey will be replaced with the current
    /// program ID.
    pub fn accounts(
        &self,
        accounts: impl ToAccountMetas,
        convert_optional: bool,
    ) -> Vec<AccountMeta> {
        if convert_optional {
            fix_optional_account_metas(accounts, &gmsol_programs::gmsol_store::ID, &self.id)
        } else {
            accounts.to_account_metas(None)
        }
    }

    /// Create an instruction builder.
    pub fn instruction(&self, args: impl InstructionData) -> InstructionBuilder<'_> {
        InstructionBuilder {
            program: self,
            data: args.data(),
            accounts: vec![],
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
        pda::find_order_address(&self.store, owner, nonce, &self.id).0
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
}

/// Buidler for Store Program Instruction.
pub struct InstructionBuilder<'a> {
    program: &'a StoreProgram,
    data: Vec<u8>,
    accounts: Vec<AccountMeta>,
}

impl InstructionBuilder<'_> {
    /// Append accounts.
    pub fn accounts(mut self, accounts: impl ToAccountMetas, convert_optional: bool) -> Self {
        let mut accounts = self.program.accounts(accounts, convert_optional);
        self.accounts.append(&mut accounts);
        self
    }

    /// Build.
    pub fn build(self) -> Instruction {
        Instruction {
            program_id: self.program.id.0,
            accounts: self.accounts,
            data: self.data,
        }
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
