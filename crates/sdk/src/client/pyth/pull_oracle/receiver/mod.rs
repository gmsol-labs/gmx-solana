use std::{ops::Deref, sync::Arc};

use gmsol_solana_utils::{
    compute_budget::ComputeBudget, program::Program, transaction_builder::TransactionBuilder,
};
use pythnet_sdk::wire::v1::MerklePriceUpdate;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, system_program};

mod accounts;
mod instruction;

/// Treasury account seed.
pub const TREASURY_SEED: &[u8] = b"treasury";

/// Config account seed.
pub const CONFIG_SEED: &[u8] = b"config";

/// `post_price_update` compute budget.
pub const POST_PRICE_UPDATE_COMPUTE_BUDGET: u32 = 80_000;

/// `reclaim_rent` compute budget.
pub const RECLAIM_RENT_COMPUTE_BUDGET: u32 = 4_000;

/// Find PDA for treasury account.
pub fn find_treasury_pda(treasury_id: u8) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TREASURY_SEED, &[treasury_id]],
        &pyth_solana_receiver_sdk::ID,
    )
}

/// Find PDA for config account.
pub fn find_config_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED], &pyth_solana_receiver_sdk::ID)
}

/// Pyth Receiver Ops.
pub trait PythReceiverOps<C> {
    /// Post price update.
    fn post_price_update<'a>(
        &'a self,
        price_update: Keypair,
        update: &MerklePriceUpdate,
        encoded_vaa: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'a, C, Pubkey>>;

    /// Reclaim rent.
    fn reclaim_rent(&self, price_update: &Pubkey) -> TransactionBuilder<C>;
}

impl<S, C> PythReceiverOps<C> for Program<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn post_price_update<'a>(
        &'a self,
        price_update: Keypair,
        update: &MerklePriceUpdate,
        encoded_vaa: &Pubkey,
    ) -> crate::Result<TransactionBuilder<'a, C, Pubkey>> {
        let treasury_id = rand::random();
        Ok(self
            .transaction()
            .output(price_update.pubkey())
            .anchor_args(instruction::PostUpdate {
                merkle_price_update: update.clone(),
                treasury_id,
            })
            .anchor_accounts(accounts::PostUpdate {
                payer: self.payer(),
                encoded_vaa: *encoded_vaa,
                config: find_config_pda().0,
                treasury: find_treasury_pda(treasury_id).0,
                price_update_account: price_update.pubkey(),
                system_program: system_program::ID,
                write_authority: self.payer(),
            })
            .owned_signer(Arc::new(price_update))
            .compute_budget(ComputeBudget::default().with_limit(POST_PRICE_UPDATE_COMPUTE_BUDGET)))
    }

    fn reclaim_rent(&self, price_update: &Pubkey) -> TransactionBuilder<C> {
        self.transaction()
            .anchor_args(instruction::ReclaimRent {})
            .anchor_accounts(accounts::ReclaimRent {
                payer: self.payer(),
                price_update_account: *price_update,
            })
            .compute_budget(ComputeBudget::default().with_limit(RECLAIM_RENT_COMPUTE_BUDGET))
    }
}
