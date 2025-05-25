use std::{future::Future, ops::Deref};

use gmsol_programs::gmsol_store::{
    accounts::Oracle,
    client::{accounts, args},
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::oracle::PriceProviderKind;
use solana_sdk::{
    pubkey::Pubkey, signer::Signer, system_instruction::create_account, system_program,
};

/// Operations for oracle management.
pub trait OracleOps<C> {
    /// Initialize [`Oracle`] account.
    fn initialize_oracle<'a>(
        &'a self,
        store: &Pubkey,
        oracle: &'a dyn Signer,
        authority: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<(TransactionBuilder<'a, C>, Pubkey)>>;

    /// Initialize Price Feed.
    fn initialize_price_feed(
        &self,
        store: &Pubkey,
        index: u16,
        provider: PriceProviderKind,
        token: &Pubkey,
        feed_id: &Pubkey,
    ) -> (TransactionBuilder<C>, Pubkey);

    /// Update price feed with chainlink.
    #[cfg(feature = "gmsol-chainlink-datastreams")]
    fn update_price_feed_with_chainlink(
        &self,
        store: &Pubkey,
        price_feed: &Pubkey,
        chainlink: &Pubkey,
        access_controller: &Pubkey,
        signed_report: &[u8],
    ) -> crate::Result<TransactionBuilder<C>>;
}

impl<C: Deref<Target = impl Signer> + Clone> OracleOps<C> for crate::Client<C> {
    async fn initialize_oracle<'a>(
        &'a self,
        store: &Pubkey,
        oracle: &'a dyn Signer,
        authority: Option<&Pubkey>,
    ) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let payer = self.payer();
        let oracle_address = oracle.pubkey();

        let size = 8 + std::mem::size_of::<Oracle>();
        let lamports = self
            .store_program()
            .rpc()
            .get_minimum_balance_for_rent_exemption(size)
            .await
            .map_err(crate::Error::custom)?;
        let create = create_account(
            &payer,
            &oracle_address,
            lamports,
            size as u64,
            self.store_program_id(),
        );

        let builder = self
            .store_transaction()
            .pre_instruction(create, false)
            .anchor_accounts(accounts::InitializeOracle {
                payer,
                authority: authority.copied().unwrap_or(payer),
                store: *store,
                oracle: oracle_address,
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializeOracle {})
            .signer(oracle);
        Ok((builder, oracle_address))
    }

    fn initialize_price_feed(
        &self,
        store: &Pubkey,
        index: u16,
        provider: PriceProviderKind,
        token: &Pubkey,
        feed_id: &Pubkey,
    ) -> (TransactionBuilder<C>, Pubkey) {
        let authority = self.payer();
        let price_feed = self.find_price_feed_address(store, &authority, index, provider, token);
        let rpc = self
            .store_transaction()
            .anchor_accounts(accounts::InitializePriceFeed {
                authority,
                store: *store,
                price_feed,
                system_program: system_program::ID,
            })
            .anchor_args(args::InitializePriceFeed {
                index,
                provider: provider.into(),
                token: *token,
                feed_id: *feed_id,
            });
        (rpc, price_feed)
    }

    #[cfg(feature = "gmsol-chainlink-datastreams")]
    fn update_price_feed_with_chainlink(
        &self,
        store: &Pubkey,
        price_feed: &Pubkey,
        chainlink: &Pubkey,
        access_controller: &Pubkey,
        signed_report: &[u8],
    ) -> crate::Result<TransactionBuilder<C>> {
        use gmsol_chainlink_datastreams::utils::{
            find_config_account_pda, find_verifier_account_pda, Compressor,
        };

        let authority = self.payer();
        let verifier_account = find_verifier_account_pda(chainlink);
        let config_account = find_config_account_pda(signed_report, chainlink);
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::UpdatePriceFeedWithChainlink {
                authority,
                store: *store,
                verifier_account,
                access_controller: *access_controller,
                config_account,
                price_feed: *price_feed,
                chainlink: *chainlink,
            })
            .anchor_args(args::UpdatePriceFeedWithChainlink {
                compressed_report: Compressor::compress(signed_report)
                    .map_err(crate::Error::custom)?,
            }))
    }
}
