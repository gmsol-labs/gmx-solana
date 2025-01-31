use std::{future::Future, ops::Deref};

use anchor_client::{
    solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction},
    ClientError,
};
use gmsol_solana_utils::{
    compute_budget::ComputeBudget, program::Program, transaction_builder::TransactionBuilder,
};

mod accounts;
mod instruction;

/// Wormhole Core Bridge Program Address.
pub const WORMHOLE_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    241, 11, 180, 229, 13, 86, 253, 161, 61, 254, 31, 50, 155, 141, 57, 61, 210, 74, 1, 69, 145,
    225, 131, 22, 151, 148, 13, 124, 52, 163, 141, 221,
]);

/// The start offset of the VAA bytes.
pub const VAA_START: u64 = 46;

/// Seed for guardian set account.
pub const GUARDIAN_SET_SEED: &[u8] = b"GuardianSet";

/// `init_encoded_vaa` compute budget.
pub const INIT_ENCODED_VAA_COMPUTE_BUDGET: u32 = 3_000;

/// `write_encoded_vaa` compute budget.
pub const WRITE_ENCODED_VAA_COMPUTE_BUDGET: u32 = 3_000;

/// `verify_encoded_vaa_v1` compute budget.
pub const VERIFY_ENCODED_VAA_V1_COMPUTE_BUDGET: u32 = 350_000;

/// `close_encoded_vaa` compute budget.
pub const CLOSE_ENCODED_VAA_COMPUTE_BUDGET: u32 = 3_000;

/// Find PDA for guardian set.
pub fn find_guardian_set_pda(guardian_set_index: i32) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[GUARDIAN_SET_SEED, &guardian_set_index.to_be_bytes()],
        &WORMHOLE_PROGRAM_ID,
    )
}

/// Wormhole Ops.
pub trait WormholeOps<C> {
    /// Create and initialize an encoded vaa account.
    fn create_encoded_vaa(
        &self,
        encoded_vaa: Keypair,
        vaa_buffer_len: u64,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C, Pubkey>>>;

    /// Write to encoded vaa account.
    fn write_encoded_vaa(
        &self,
        draft_vaa: &Pubkey,
        index: u32,
        data: &[u8],
    ) -> TransactionBuilder<C>;

    /// Verify encoded vaa account.
    fn verify_encoded_vaa_v1(
        &self,
        draft_vaa: &Pubkey,
        guardian_set_index: i32,
    ) -> TransactionBuilder<C>;

    /// Close encoded vaa account.
    fn close_encoded_vaa(&self, encoded_vaa: &Pubkey) -> TransactionBuilder<C>;
}

impl<S, C> WormholeOps<C> for Program<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    async fn create_encoded_vaa(
        &self,
        encoded_vaa: Keypair,
        vaa_buffer_len: u64,
    ) -> crate::Result<TransactionBuilder<C, Pubkey>> {
        let space = vaa_buffer_len + VAA_START;
        let lamports = self
            .rpc()
            .get_minimum_balance_for_rent_exemption(space as usize)
            .await
            .map_err(ClientError::from)?;
        let request = self
            .transaction()
            .pre_instruction(system_instruction::create_account(
                &self.payer(),
                &encoded_vaa.pubkey(),
                lamports,
                space,
                self.id(),
            ))
            .anchor_args(instruction::InitEncodedVaa {})
            .anchor_accounts(accounts::InitEncodedVaa {
                write_authority: self.payer(),
                encoded_vaa: encoded_vaa.pubkey(),
            })
            .output(encoded_vaa.pubkey())
            .owned_signer(encoded_vaa)
            .compute_budget(ComputeBudget::default().with_limit(INIT_ENCODED_VAA_COMPUTE_BUDGET));
        Ok(request)
    }

    fn write_encoded_vaa(
        &self,
        draft_vaa: &Pubkey,
        index: u32,
        data: &[u8],
    ) -> TransactionBuilder<C> {
        self.transaction()
            .anchor_args(instruction::WriteEncodedVaa {
                index,
                data: data.to_owned(),
            })
            .anchor_accounts(accounts::WriteEncodedVaa {
                write_authority: self.payer(),
                draft_vaa: *draft_vaa,
            })
            .compute_budget(ComputeBudget::default().with_limit(WRITE_ENCODED_VAA_COMPUTE_BUDGET))
    }

    fn verify_encoded_vaa_v1(
        &self,
        draft_vaa: &Pubkey,
        guardian_set_index: i32,
    ) -> TransactionBuilder<C> {
        self.transaction()
            .anchor_args(instruction::VerifyEncodedVaaV1 {})
            .anchor_accounts(accounts::VerifyEncodedVaaV1 {
                write_authority: self.payer(),
                draft_vaa: *draft_vaa,
                guardian_set: find_guardian_set_pda(guardian_set_index).0,
            })
            .compute_budget(
                ComputeBudget::default().with_limit(VERIFY_ENCODED_VAA_V1_COMPUTE_BUDGET),
            )
    }

    fn close_encoded_vaa(&self, encoded_vaa: &Pubkey) -> TransactionBuilder<C> {
        self.transaction()
            .anchor_args(instruction::CloseEncodedVaa {})
            .anchor_accounts(accounts::CloseEncodedVaa {
                write_authority: self.payer(),
                encoded_vaa: *encoded_vaa,
            })
            .compute_budget(ComputeBudget::default().with_limit(CLOSE_ENCODED_VAA_COMPUTE_BUDGET))
    }
}
