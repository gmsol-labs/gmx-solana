use std::{future::Future, ops::Deref};

use gmsol_programs::gmsol_store::{
    accounts::{ReferralCodeV2, UserHeader},
    client::{accounts, args},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::pubkey::optional_address;
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

use crate::{pda::ReferralCodeBytes, utils::zero_copy::ZeroCopy};

/// Operations for user account.
pub trait UserOps<C> {
    /// Replace the payer's GT delegate and remaining allowance together.
    /// Pass `Pubkey::default()` and zero to clear the delegate.
    fn set_gt_delegate(
        &self,
        store: &Pubkey,
        delegate: &Pubkey,
        allowance: u64,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Add to the payer's GT allowance at execution time.
    fn increase_gt_allowance(
        &self,
        store: &Pubkey,
        amount: u64,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Subtract from the payer's GT allowance at execution time, stopping at zero.
    fn decrease_gt_allowance(
        &self,
        store: &Pubkey,
        amount: u64,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Prepare User.
    fn prepare_user(&self, store: &Pubkey) -> crate::Result<TransactionBuilder<C>>;

    /// Initialize Referral Code.
    fn initialize_referral_code(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
    ) -> crate::Result<TransactionBuilder<C>>;

    /// Set referrer.
    fn set_referrer(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
        hint_referrer: Option<Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Transfer referral code.
    fn transfer_referral_code(
        &self,
        store: &Pubkey,
        receiver: &Pubkey,
        hint_code: Option<ReferralCodeBytes>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Cancel referral code transfer.
    fn cancel_referral_code_transfer(
        &self,
        store: &Pubkey,
        hint_code: Option<ReferralCodeBytes>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Accept referral code transfer.
    fn accept_referral_code(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
        hint_owner: Option<Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

    /// Set the builder fee factor advertised by the payer's user account.
    ///
    /// The factor must not exceed the store's `MaxBuilderFeeFactor`, which
    /// reads `0` until a config keeper raises it. Passing `0` opts out.
    fn set_builder_fee_factor(
        &self,
        store: &Pubkey,
        factor: u128,
    ) -> crate::Result<TransactionBuilder<C>>;
}

impl<C: Deref<Target = impl Signer> + Clone> UserOps<C> for crate::Client<C> {
    fn set_gt_delegate(
        &self,
        store: &Pubkey,
        delegate: &Pubkey,
        allowance: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::SetGtDelegate {
                owner: self.payer(),
                store: *store,
                user: self.find_user_address(store, &self.payer()),
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::SetGtDelegate {
                delegate: *delegate,
                allowance,
            }))
    }

    fn increase_gt_allowance(
        &self,
        store: &Pubkey,
        amount: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::IncreaseGtAllowance {
                owner: self.payer(),
                store: *store,
                user: self.find_user_address(store, &self.payer()),
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::IncreaseGtAllowance { amount }))
    }

    fn decrease_gt_allowance(
        &self,
        store: &Pubkey,
        amount: u64,
    ) -> crate::Result<TransactionBuilder<C>> {
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::DecreaseGtAllowance {
                owner: self.payer(),
                store: *store,
                user: self.find_user_address(store, &self.payer()),
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::DecreaseGtAllowance { amount }))
    }

    fn prepare_user(&self, store: &Pubkey) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::PrepareUser {
                owner,
                store: *store,
                user,
                system_program: system_program::ID,
            })
            .anchor_args(args::PrepareUser {});
        Ok(rpc)
    }

    fn initialize_referral_code(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let referral_code = self.find_referral_code_address(store, code);
        let user = self.find_user_address(store, &owner);
        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::InitializeReferralCode {
                owner,
                store: *store,
                referral_code,
                user,
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializeReferralCode { code });
        Ok(rpc)
    }

    async fn set_referrer(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
        hint_referrer_user: Option<Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);

        let referral_code = self.find_referral_code_address(store, code);

        let referrer_user = match hint_referrer_user {
            Some(referrer) => referrer,
            None => {
                let code = self
                    .account::<ZeroCopy<ReferralCodeV2>>(&referral_code)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let owner = code.owner;
                self.find_user_address(store, &owner)
            }
        };

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::SetReferrer {
                owner,
                store: *store,
                user,
                referral_code,
                referrer_user,
            })
            .anchor_args(args::SetReferrer { code });

        Ok(rpc)
    }

    async fn transfer_referral_code(
        &self,
        store: &Pubkey,
        receiver: &Pubkey,
        hint_code: Option<ReferralCodeBytes>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let receiver_user = self.find_user_address(store, receiver);

        let referral_code = match hint_code {
            Some(code) => self.find_referral_code_address(store, code),
            None => {
                let user = self
                    .account::<ZeroCopy<UserHeader>>(&user)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                *optional_address(&user.0.referral.code)
                    .ok_or(crate::Error::custom("referral code is not set"))?
            }
        };

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::TransferReferralCode {
                owner,
                store: *store,
                user,
                referral_code,
                receiver_user,
            })
            .anchor_args(args::TransferReferralCode {});

        Ok(rpc)
    }

    async fn cancel_referral_code_transfer(
        &self,
        store: &Pubkey,
        hint_code: Option<ReferralCodeBytes>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);

        let referral_code = match hint_code {
            Some(code) => self.find_referral_code_address(store, code),
            None => {
                let user = self
                    .account::<ZeroCopy<UserHeader>>(&user)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                *optional_address(&user.0.referral.code)
                    .ok_or(crate::Error::custom("referral code is not set"))?
            }
        };

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::CancelReferralCodeTransfer {
                owner,
                store: *store,
                user,
                referral_code,
            })
            .anchor_args(args::CancelReferralCodeTransfer {});

        Ok(rpc)
    }

    async fn accept_referral_code(
        &self,
        store: &Pubkey,
        code: ReferralCodeBytes,
        hint_owner: Option<Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let next_owner = self.payer();
        let receiver_user = self.find_user_address(store, &next_owner);
        let referral_code = self.find_referral_code_address(store, code);

        let owner = match hint_owner {
            Some(owner) => owner,
            None => {
                let code = self
                    .account::<ZeroCopy<ReferralCodeV2>>(&referral_code)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                code.owner
            }
        };

        let user = self.find_user_address(store, &owner);

        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::AcceptReferralCode {
                next_owner,
                store: *store,
                user,
                referral_code,
                receiver_user,
            })
            .anchor_args(args::AcceptReferralCode {});
        Ok(rpc)
    }

    fn set_builder_fee_factor(
        &self,
        store: &Pubkey,
        factor: u128,
    ) -> crate::Result<TransactionBuilder<C>> {
        let owner = self.payer();
        let user = self.find_user_address(store, &owner);
        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::SetBuilderFeeFactor {
                owner,
                store: *store,
                user,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
            })
            .anchor_args(args::SetBuilderFeeFactor { factor });
        Ok(rpc)
    }
}
