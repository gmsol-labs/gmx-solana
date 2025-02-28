use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_store::{
    accounts, instruction,
    states::user::{ReferralCodeBytes, ReferralCodeV2, UserHeader},
};

use crate::utils::ZeroCopy;

/// User Account Operations.
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
            .anchor_args(instruction::PrepareUser {});
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
            .anchor_args(instruction::InitializeReferralCode { code });
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
            .anchor_args(instruction::SetReferrer { code });

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
                *user
                    .0
                    .referral()
                    .code()
                    .ok_or(crate::Error::invalid_argument("referral code is not set"))?
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
            .anchor_args(instruction::TransferReferralCode {});

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
                *user
                    .0
                    .referral()
                    .code()
                    .ok_or(crate::Error::invalid_argument("referral code is not set"))?
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
            .anchor_args(instruction::CancelReferralCodeTransfer {});

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
            .anchor_args(instruction::AcceptReferralCode {});
        Ok(rpc)
    }
}
