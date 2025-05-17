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
}

impl<C: Deref<Target = impl Signer> + Clone> UserOps<C> for crate::Client<C> {
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
}
