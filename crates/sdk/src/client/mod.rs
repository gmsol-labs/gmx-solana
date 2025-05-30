/// Client for transaction subscription.
pub mod pubsub;

/// Utilities for program accounts.
pub mod accounts;

/// Utilities for transaction history.
pub mod transaction_history;

/// Definition of [`TokenMap`].
pub mod token_map;

/// Operations.
pub mod ops;

/// Feeds parser.
pub mod feeds_parser;

/// Pull oracle support.
pub mod pull_oracle;

/// Utilities for token accounts.
pub mod token_account;

/// Simulate a transaction and view its output.
pub mod view;

/// Instruction buffer.
pub mod instruction_buffer;

/// Program IDs.
pub mod program_ids;

/// Chainlink support.
#[cfg(feature = "chainlink")]
pub mod chainlink;

/// Pyth support.
#[cfg(feature = "pyth")]
pub mod pyth;

/// Switchboard support.
#[cfg(feature = "switchboard")]
pub mod switchboard;

/// Squads operations.
#[cfg(feature = "squads")]
pub mod squads;

use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{Arc, OnceLock},
};

use accounts::{
    account_with_context, accounts_lazy_with_context, get_account_with_context,
    ProgramAccountsConfig,
};
use gmsol_model::{price::Prices, PnlFactorKind};
use gmsol_programs::{
    anchor_lang::{AccountDeserialize, AnchorSerialize, Discriminator},
    bytemuck,
    gmsol_store::{
        accounts as store_accounts,
        types::{self as store_types, MarketStatus},
    },
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions, CreateBundleOptions},
    cluster::Cluster,
    program::Program,
    transaction_builder::{default_before_sign, Config, TransactionBuilder},
    utils::WithSlot,
};
use gmsol_utils::oracle::PriceProviderKind;
use instruction_buffer::InstructionBuffer;
use ops::market::MarketOps;
use pubsub::{PubsubClient, SubscriptionConfig};
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::RpcAccountInfoConfig,
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey, signer::Signer,
};
use token_map::TokenMap;
use tokio::sync::OnceCell;
use typed_builder::TypedBuilder;

use crate::{
    builders::callback::{Callback, CallbackParams},
    pda::{NonceBytes, ReferralCodeBytes},
    utils::{
        optional::optional_address,
        zero_copy::{SharedZeroCopy, ZeroCopy},
    },
};

#[cfg(feature = "decode")]
use gmsol_decode::{gmsol::programs::GMSOLCPIEvent, Decode};

#[cfg(feature = "decode")]
use gmsol_programs::gmsol_store::events as store_events;

const DISC_OFFSET: usize = 8;

/// Options for [`Client`].
#[derive(Debug, Clone, TypedBuilder)]
pub struct ClientOptions {
    #[builder(default)]
    store_program_id: Option<Pubkey>,
    #[builder(default)]
    treasury_program_id: Option<Pubkey>,
    #[builder(default)]
    timelock_program_id: Option<Pubkey>,
    #[builder(default)]
    commitment: CommitmentConfig,
    #[builder(default)]
    subscription: SubscriptionConfig,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Client for interacting with the GMX-Solana protocol.
pub struct Client<C> {
    cfg: Config<C>,
    store_program: Program<C>,
    treasury_program: Program<C>,
    timelock_program: Program<C>,
    rpc: OnceLock<RpcClient>,
    pub_sub: OnceCell<PubsubClient>,
    subscription_config: SubscriptionConfig,
}

impl<C: Clone + Deref<Target = impl Signer>> Client<C> {
    /// Create a new [`Client`] with the given options.
    pub fn new_with_options(
        cluster: Cluster,
        payer: C,
        options: ClientOptions,
    ) -> crate::Result<Self> {
        let ClientOptions {
            store_program_id,
            treasury_program_id,
            timelock_program_id,
            commitment,
            subscription,
        } = options;
        let cfg = Config::new(cluster, payer, commitment);
        Ok(Self {
            store_program: Program::new(
                store_program_id.unwrap_or(gmsol_programs::gmsol_store::ID),
                cfg.clone(),
            ),
            treasury_program: Program::new(
                treasury_program_id.unwrap_or(gmsol_programs::gmsol_treasury::ID),
                cfg.clone(),
            ),
            timelock_program: Program::new(
                timelock_program_id.unwrap_or(gmsol_programs::gmsol_timelock::ID),
                cfg.clone(),
            ),
            cfg,
            pub_sub: OnceCell::default(),
            rpc: Default::default(),
            subscription_config: subscription,
        })
    }

    /// Create a new [`Client`] with default options.
    pub fn new(cluster: Cluster, payer: C) -> crate::Result<Self> {
        Self::new_with_options(cluster, payer, ClientOptions::default())
    }

    /// Create a clone of this client with a new payer.
    pub fn try_clone_with_payer<C2: Clone + Deref<Target = impl Signer>>(
        &self,
        payer: C2,
    ) -> crate::Result<Client<C2>> {
        Client::new_with_options(
            self.cluster().clone(),
            payer,
            ClientOptions {
                store_program_id: Some(*self.store_program_id()),
                treasury_program_id: Some(*self.treasury_program_id()),
                timelock_program_id: Some(*self.timelock_program_id()),
                commitment: self.commitment(),
                subscription: self.subscription_config.clone(),
            },
        )
    }

    /// Create a clone of this client.
    pub fn try_clone(&self) -> crate::Result<Self> {
        Ok(Self {
            cfg: self.cfg.clone(),
            store_program: self.program(*self.store_program_id()),
            treasury_program: self.program(*self.treasury_program_id()),
            timelock_program: self.program(*self.timelock_program_id()),
            pub_sub: OnceCell::default(),
            rpc: Default::default(),
            subscription_config: self.subscription_config.clone(),
        })
    }

    /// Replace subscription config with the given.
    pub fn set_subscription_config(&mut self, config: SubscriptionConfig) -> &mut Self {
        self.subscription_config = config;
        self
    }

    /// Create a new [`Program`] with the given program id.
    pub fn program(&self, program_id: Pubkey) -> Program<C> {
        Program::new(program_id, self.cfg.clone())
    }

    /// Get current cluster.
    pub fn cluster(&self) -> &Cluster {
        self.cfg.cluster()
    }

    /// Get current commitment config.
    pub fn commitment(&self) -> CommitmentConfig {
        *self.cfg.commitment()
    }

    /// Get current payer.
    pub fn payer(&self) -> Pubkey {
        self.cfg.payer()
    }

    /// Get [`RpcClient`].
    pub fn rpc(&self) -> &RpcClient {
        self.rpc.get_or_init(|| self.cfg.rpc())
    }

    /// Get store program.
    pub fn store_program(&self) -> &Program<C> {
        &self.store_program
    }

    /// Get treasury program.
    pub fn treasury_program(&self) -> &Program<C> {
        &self.treasury_program
    }

    /// Get timelock program.
    pub fn timelock_program(&self) -> &Program<C> {
        &self.timelock_program
    }

    /// Create a new store program.
    pub fn new_store_program(&self) -> crate::Result<Program<C>> {
        Ok(self.program(*self.store_program_id()))
    }

    /// Create a new treasury program.
    pub fn new_treasury_program(&self) -> crate::Result<Program<C>> {
        Ok(self.program(*self.store_program_id()))
    }

    /// Get the program id of the store program.
    pub fn store_program_id(&self) -> &Pubkey {
        self.store_program().id()
    }

    /// Get the program id of the treasury program.
    pub fn treasury_program_id(&self) -> &Pubkey {
        self.treasury_program().id()
    }

    /// Get the program id of the timelock program.
    pub fn timelock_program_id(&self) -> &Pubkey {
        self.timelock_program().id()
    }

    /// Create a [`TransactionBuilder`] for the store program.
    pub fn store_transaction(&self) -> TransactionBuilder<'_, C> {
        self.store_program().transaction()
    }

    /// Create a [`TransactionBuilder`] for the treasury program.
    pub fn treasury_transaction(&self) -> TransactionBuilder<'_, C> {
        self.treasury_program().transaction()
    }

    /// Create a [`TransactionBuilder`] for the timelock program.
    pub fn timelock_transaction(&self) -> TransactionBuilder<'_, C> {
        self.timelock_program().transaction()
    }

    /// Create a [`BundleBuilder`] with the given options.
    pub fn bundle_with_options(&self, options: BundleOptions) -> BundleBuilder<'_, C> {
        BundleBuilder::new_with_options(CreateBundleOptions {
            cluster: self.cluster().clone(),
            commitment: self.commitment(),
            options,
        })
    }

    /// Create a [`BundleBuilder`] with default options.
    pub fn bundle(&self) -> BundleBuilder<C> {
        self.bundle_with_options(Default::default())
    }

    /// Find PDA for [`Store`](gmsol_programs::gmsol_store::accounts::Store) account.
    pub fn find_store_address(&self, key: &str) -> Pubkey {
        crate::pda::find_store_address(key, self.store_program_id()).0
    }

    /// Find PDA for store wallet account.
    pub fn find_store_wallet_address(&self, store: &Pubkey) -> Pubkey {
        crate::pda::find_store_wallet_address(store, self.store_program_id()).0
    }

    /// Get the event authority PDA for the `Store` program.
    pub fn store_event_authority(&self) -> Pubkey {
        crate::pda::find_event_authority_address(self.store_program_id()).0
    }

    /// Find PDA for market vault account.
    pub fn find_market_vault_address(&self, store: &Pubkey, token: &Pubkey) -> Pubkey {
        crate::pda::find_market_vault_address(store, token, self.store_program_id()).0
    }

    /// Find PDA for market token mint account.
    pub fn find_market_token_address(
        &self,
        store: &Pubkey,
        index_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_market_token_address(
            store,
            index_token,
            long_token,
            short_token,
            self.store_program_id(),
        )
        .0
    }

    /// Find PDA for market account.
    pub fn find_market_address(&self, store: &Pubkey, token: &Pubkey) -> Pubkey {
        crate::pda::find_market_address(store, token, self.store_program_id()).0
    }

    /// Find PDA for deposit account.
    pub fn find_deposit_address(
        &self,
        store: &Pubkey,
        user: &Pubkey,
        nonce: &NonceBytes,
    ) -> Pubkey {
        crate::pda::find_deposit_address(store, user, nonce, self.store_program_id()).0
    }

    /// Find PDA for first deposit owner.
    pub fn find_first_deposit_owner_address(&self) -> Pubkey {
        crate::pda::find_first_deposit_receiver_address(self.store_program_id()).0
    }

    /// Find PDA for withdrawal account.
    pub fn find_withdrawal_address(
        &self,
        store: &Pubkey,
        user: &Pubkey,
        nonce: &NonceBytes,
    ) -> Pubkey {
        crate::pda::find_withdrawal_address(store, user, nonce, self.store_program_id()).0
    }

    /// Find PDA for order.
    pub fn find_order_address(&self, store: &Pubkey, user: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        crate::pda::find_order_address(store, user, nonce, self.store_program_id()).0
    }

    /// Find PDA for shift.
    pub fn find_shift_address(&self, store: &Pubkey, owner: &Pubkey, nonce: &NonceBytes) -> Pubkey {
        crate::pda::find_shift_address(store, owner, nonce, self.store_program_id()).0
    }

    /// Find PDA for position.
    pub fn find_position_address(
        &self,
        store: &Pubkey,
        user: &Pubkey,
        market_token: &Pubkey,
        collateral_token: &Pubkey,
        is_long: bool,
    ) -> crate::Result<Pubkey> {
        Ok(crate::pda::find_position_address(
            store,
            user,
            market_token,
            collateral_token,
            is_long,
            self.store_program_id(),
        )
        .0)
    }

    /// Find PDA for claimable account.
    pub fn find_claimable_account_address(
        &self,
        store: &Pubkey,
        mint: &Pubkey,
        user: &Pubkey,
        time_key: &[u8],
    ) -> Pubkey {
        crate::pda::find_claimable_account_address(
            store,
            mint,
            user,
            time_key,
            self.store_program_id(),
        )
        .0
    }

    /// Find PDA for trade event buffer account.
    pub fn find_trade_event_buffer_address(
        &self,
        store: &Pubkey,
        authority: &Pubkey,
        index: u16,
    ) -> Pubkey {
        crate::pda::find_trade_event_buffer_address(
            store,
            authority,
            index,
            self.store_program_id(),
        )
        .0
    }

    /// Find PDA for user account.
    pub fn find_user_address(&self, store: &Pubkey, owner: &Pubkey) -> Pubkey {
        crate::pda::find_user_address(store, owner, self.store_program_id()).0
    }

    /// Find PDA for referral code.
    pub fn find_referral_code_address(&self, store: &Pubkey, code: ReferralCodeBytes) -> Pubkey {
        crate::pda::find_referral_code_address(store, code, self.store_program_id()).0
    }

    /// Find PDA for GLV token mint.
    pub fn find_glv_token_address(&self, store: &Pubkey, index: u16) -> Pubkey {
        crate::pda::find_glv_token_address(store, index, self.store_program_id()).0
    }

    /// Find PDA for GLV.
    pub fn find_glv_address(&self, glv_token: &Pubkey) -> Pubkey {
        crate::pda::find_glv_address(glv_token, self.store_program_id()).0
    }

    /// Find PDA for GLV deposit.
    pub fn find_glv_deposit_address(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
        nonce: &NonceBytes,
    ) -> Pubkey {
        crate::pda::find_glv_deposit_address(store, owner, nonce, self.store_program_id()).0
    }

    /// Find PDA for GLV withdrawal.
    pub fn find_glv_withdrawal_address(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
        nonce: &NonceBytes,
    ) -> Pubkey {
        crate::pda::find_glv_withdrawal_address(store, owner, nonce, self.store_program_id()).0
    }

    /// Find PDA for GT exchange vault.
    pub fn find_gt_exchange_vault_address(
        &self,
        store: &Pubkey,
        time_window_index: i64,
        time_window: u32,
    ) -> Pubkey {
        crate::pda::find_gt_exchange_vault_address(
            store,
            time_window_index,
            time_window,
            self.store_program_id(),
        )
        .0
    }

    /// Find PDA for GT exchange.
    pub fn find_gt_exchange_address(&self, vault: &Pubkey, owner: &Pubkey) -> Pubkey {
        crate::pda::find_gt_exchange_address(vault, owner, self.store_program_id()).0
    }

    /// Find PDA for custom price feed.
    pub fn find_price_feed_address(
        &self,
        store: &Pubkey,
        authority: &Pubkey,
        index: u16,
        provider: PriceProviderKind,
        token: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_price_feed_address(
            store,
            authority,
            index,
            provider,
            token,
            self.store_program_id(),
        )
        .0
    }

    /// Find PDA for treasury global config.
    pub fn find_treasury_config_address(&self, store: &Pubkey) -> Pubkey {
        crate::pda::find_treasury_config_address(store, self.treasury_program_id()).0
    }

    /// Find PDA for treasury vault config.
    pub fn find_treasury_vault_config_address(&self, config: &Pubkey, index: u16) -> Pubkey {
        crate::pda::find_treasury_vault_config_address(config, index, self.treasury_program_id()).0
    }

    /// Find PDA for GT bank.
    pub fn find_gt_bank_address(
        &self,
        treasury_vault_config: &Pubkey,
        gt_exchange_vault: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_gt_bank_address(
            treasury_vault_config,
            gt_exchange_vault,
            self.treasury_program_id(),
        )
        .0
    }

    /// Find PDA for treasury receiver.
    pub fn find_treasury_receiver_address(&self, config: &Pubkey) -> Pubkey {
        crate::pda::find_treasury_receiver_address(config, self.treasury_program_id()).0
    }

    /// Find PDA for timelock config.
    pub fn find_timelock_config_address(&self, store: &Pubkey) -> Pubkey {
        crate::pda::find_timelock_config_address(store, self.timelock_program_id()).0
    }

    /// Find PDA for timelock executor.
    pub fn find_executor_address(&self, store: &Pubkey, role: &str) -> crate::Result<Pubkey> {
        Ok(crate::pda::find_executor_address(store, role, self.timelock_program_id())?.0)
    }

    /// Find the wallet PDA for the given timelock executor.
    pub fn find_executor_wallet_address(&self, executor: &Pubkey) -> Pubkey {
        crate::pda::find_executor_wallet_address(executor, self.timelock_program_id()).0
    }

    /// Find the PDA for callback authority.
    pub fn find_callback_authority_address(&self) -> Pubkey {
        crate::pda::find_callback_authority(self.store_program_id()).0
    }

    /// Find the PDA for virtual inventory for swaps.
    pub fn find_virtual_inventory_for_swaps_address(&self, store: &Pubkey, index: u32) -> Pubkey {
        crate::pda::find_virtual_inventory_for_swaps_address(store, index, self.store_program_id())
            .0
    }

    /// Find the PDA for virtual inventory for positions.
    pub fn find_virtual_inventory_for_positions_address(
        &self,
        store: &Pubkey,
        index_token: &Pubkey,
    ) -> Pubkey {
        crate::pda::find_virtual_inventory_for_positions_address(
            store,
            index_token,
            self.store_program_id(),
        )
        .0
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

    /// Get latest slot.
    pub async fn get_slot(&self, commitment: Option<CommitmentConfig>) -> crate::Result<u64> {
        let slot = self
            .store_program()
            .rpc()
            .get_slot_with_commitment(commitment.unwrap_or(self.commitment()))
            .await
            .map_err(crate::Error::custom)?;
        Ok(slot)
    }

    /// Fetch accounts owned by the store program.
    pub async fn store_accounts_with_config<T>(
        &self,
        filter_by_store: Option<StoreFilter>,
        other_filters: impl IntoIterator<Item = RpcFilterType>,
        config: ProgramAccountsConfig,
    ) -> crate::Result<WithSlot<Vec<(Pubkey, T)>>>
    where
        T: AccountDeserialize + Discriminator,
    {
        let filters = std::iter::empty()
            .chain(
                filter_by_store
                    .inspect(|filter| {
                        let store = &filter.store;
                        tracing::debug!(%store, offset=%filter.store_offset(), "store bytes to filter: {}", hex::encode(store));
                    })
                    .map(RpcFilterType::from),
            )
            .chain(other_filters);
        accounts_lazy_with_context(self.store_program(), filters, config)
            .await?
            .map(|iter| iter.collect())
            .transpose()
    }

    /// Fetch account without deserialization.
    pub async fn raw_account_with_config(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<Account>>> {
        let client = self.store_program().rpc();
        get_account_with_context(&client, address, config).await
    }

    /// Fetch account and decode.
    #[cfg(feature = "decode")]
    pub async fn decode_account_with_config(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<gmsol_decode::gmsol::programs::GMSOLAccountData>>> {
        use crate::utils::decode::KeyedAccount;
        use gmsol_decode::{decoder::AccountAccessDecoder, gmsol::programs::GMSOLAccountData};

        let account = self.raw_account_with_config(address, config).await?;
        let slot = account.slot();
        match account.into_value() {
            Some(account) => {
                let account = KeyedAccount {
                    pubkey: *address,
                    account: WithSlot::new(slot, account),
                };
                let decoder = AccountAccessDecoder::new(account);
                let decoded = GMSOLAccountData::decode(decoder)?;
                Ok(WithSlot::new(slot, Some(decoded)))
            }
            None => Ok(WithSlot::new(slot, None)),
        }
    }

    /// Fetch account with the given address with config.
    ///
    /// The value inside the returned context will be `None` if the account does not exist.
    pub async fn account_with_config<T>(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<T>>>
    where
        T: AccountDeserialize,
    {
        let client = self.store_program().rpc();
        account_with_context(&client, address, config).await
    }

    /// Fetch account with the given address.
    pub async fn account<T: AccountDeserialize>(
        &self,
        address: &Pubkey,
    ) -> crate::Result<Option<T>> {
        Ok(self
            .account_with_config(address, Default::default())
            .await?
            .into_value())
    }

    /// Fetch accounts owned by the store program.
    pub async fn store_accounts<T>(
        &self,
        filter_by_store: Option<StoreFilter>,
        other_filters: impl IntoIterator<Item = RpcFilterType>,
    ) -> crate::Result<Vec<(Pubkey, T)>>
    where
        T: AccountDeserialize + Discriminator,
    {
        let res = self
            .store_accounts_with_config(
                filter_by_store,
                other_filters,
                ProgramAccountsConfig::default(),
            )
            .await?;
        tracing::debug!(slot=%res.slot(), "accounts fetched");
        Ok(res.into_value())
    }

    /// Fetch [`Store`](store_accounts::Store) account with its address.
    pub async fn store(&self, address: &Pubkey) -> crate::Result<Arc<store_accounts::Store>> {
        Ok(self
            .account::<SharedZeroCopy<store_accounts::Store>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch user account with its address.
    pub async fn user(&self, address: &Pubkey) -> crate::Result<store_accounts::UserHeader> {
        Ok(self
            .account::<ZeroCopy<store_accounts::UserHeader>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch the [`TokenMap`] address of the given store.
    pub async fn authorized_token_map_address(
        &self,
        store: &Pubkey,
    ) -> crate::Result<Option<Pubkey>> {
        let store = self.store(store).await?;
        let token_map = store.token_map;
        Ok(optional_address(&token_map).copied())
    }

    /// Fetch [`TokenMap`] account with its address.
    pub async fn token_map(&self, address: &Pubkey) -> crate::Result<TokenMap> {
        self.account(address).await?.ok_or(crate::Error::NotFound)
    }

    /// Fetch the authorized token map of the given store.
    pub async fn authorized_token_map(&self, store: &Pubkey) -> crate::Result<TokenMap> {
        let address = self
            .authorized_token_map_address(store)
            .await?
            .ok_or(crate::Error::custom("token map is not set"))?;
        self.token_map(&address).await
    }

    /// Fetch all [`Market`](store_accounts::Market) accounts of the given store.
    pub async fn markets_with_config(
        &self,
        store: &Pubkey,
        config: ProgramAccountsConfig,
    ) -> crate::Result<WithSlot<BTreeMap<Pubkey, Arc<store_accounts::Market>>>> {
        let markets = self
            .store_accounts_with_config::<SharedZeroCopy<store_accounts::Market>>(
                Some(StoreFilter::new(
                    store,
                    bytemuck::offset_of!(store_accounts::Market, store),
                )),
                None,
                config,
            )
            .await?
            .map(|accounts| {
                accounts
                    .into_iter()
                    .map(|(pubkey, m)| (pubkey, m.0))
                    .collect::<BTreeMap<_, _>>()
            });
        Ok(markets)
    }

    /// Fetch all [`Market`](store_accounts::Market) accounts of the given store.
    pub async fn markets(
        &self,
        store: &Pubkey,
    ) -> crate::Result<BTreeMap<Pubkey, Arc<store_accounts::Market>>> {
        let markets = self
            .markets_with_config(store, ProgramAccountsConfig::default())
            .await?
            .into_value();
        Ok(markets)
    }

    /// Fetch [`Market`](store_accounts::Market) at the given address with config.
    ///
    /// The value inside the returned context will be `None` if the account does not exist.
    pub async fn market_with_config<T>(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<Arc<store_accounts::Market>>>> {
        let market = self
            .account_with_config::<SharedZeroCopy<store_accounts::Market>>(address, config)
            .await?;
        Ok(market.map(|m| m.map(|m| m.0)))
    }

    /// Fetch [`Market`](store_accounts::Market) account with its address.
    pub async fn market(&self, address: &Pubkey) -> crate::Result<Arc<store_accounts::Market>> {
        Ok(self
            .account::<SharedZeroCopy<store_accounts::Market>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch [`Market`](store_accounts::Market) account with its token address.
    pub async fn market_by_token(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
    ) -> crate::Result<Arc<store_accounts::Market>> {
        let address = self.find_market_address(store, market_token);
        self.market(&address).await
    }

    /// Fetch all [`Glv`](store_accounts::Glv) accounts of the given store.
    pub async fn glvs_with_config(
        &self,
        store: &Pubkey,
        config: ProgramAccountsConfig,
    ) -> crate::Result<WithSlot<BTreeMap<Pubkey, store_accounts::Glv>>> {
        let glvs = self
            .store_accounts_with_config::<ZeroCopy<store_accounts::Glv>>(
                Some(StoreFilter::new(
                    store,
                    bytemuck::offset_of!(store_accounts::Glv, store),
                )),
                None,
                config,
            )
            .await?
            .map(|accounts| {
                accounts
                    .into_iter()
                    .map(|(pubkey, m)| (pubkey, m.0))
                    .collect::<BTreeMap<_, _>>()
            });
        Ok(glvs)
    }

    /// Fetch all [`Glv`](store_accounts::Glv) accounts of the given store.
    pub async fn glvs(
        &self,
        store: &Pubkey,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Glv>> {
        let glvs = self
            .glvs_with_config(store, ProgramAccountsConfig::default())
            .await?
            .into_value();
        Ok(glvs)
    }

    /// Fetch [`MarketStatus`] with market token address.
    pub async fn market_status(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        maximize_pnl: bool,
        maximize_pool_value: bool,
    ) -> crate::Result<MarketStatus> {
        let req = self.get_market_status(
            store,
            market_token,
            prices,
            maximize_pnl,
            maximize_pool_value,
        );
        let status = view::view::<MarketStatus>(
            &self.store_program().rpc(),
            &req.signed_transaction_with_options(true, None, None, default_before_sign)
                .await?,
        )
        .await?;
        Ok(status)
    }

    /// Fetch current market token price with market token address.
    pub async fn market_token_price(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        prices: Prices<u128>,
        pnl_factor: PnlFactorKind,
        maximize: bool,
    ) -> crate::Result<u128> {
        let req = self.get_market_token_price(store, market_token, prices, pnl_factor, maximize);
        let price = view::view::<u128>(
            &self.store_program().rpc(),
            &req.signed_transaction_with_options(true, None, None, default_before_sign)
                .await?,
        )
        .await?;
        Ok(price)
    }

    /// Fetch [`Position`](store_accounts::Position) accounts.
    pub async fn positions(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Position>> {
        let filter = match owner {
            Some(owner) => {
                let mut bytes = owner.as_ref().to_owned();
                if let Some(market_token) = market_token {
                    bytes.extend_from_slice(market_token.as_ref());
                }
                let filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                    bytemuck::offset_of!(store_accounts::Position, owner) + DISC_OFFSET,
                    &bytes,
                ));
                Some(filter)
            }
            None => market_token.and_then(|token| {
                Some(RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                    bytemuck::offset_of!(store_accounts::Position, market_token) + DISC_OFFSET,
                    &token.try_to_vec().ok()?,
                )))
            }),
        };

        let store_filter =
            StoreFilter::new(store, bytemuck::offset_of!(store_accounts::Position, store));

        let positions = self
            .store_accounts::<ZeroCopy<store_accounts::Position>>(Some(store_filter), filter)
            .await?
            .into_iter()
            .map(|(pubkey, p)| (pubkey, p.0))
            .collect();

        Ok(positions)
    }

    /// Fetch [`Position`](store_accounts::Position) account with its address.
    pub async fn position(&self, address: &Pubkey) -> crate::Result<store_accounts::Position> {
        let position = self
            .account::<ZeroCopy<store_accounts::Position>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?;
        Ok(position.0)
    }

    /// Fetch [`Order`](store_accounts::Order) account with its address.
    pub async fn order(&self, address: &Pubkey) -> crate::Result<store_accounts::Order> {
        Ok(self
            .account::<ZeroCopy<store_accounts::Order>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch [`Order`](store_accounts::Order) account at the the given address with config.
    ///
    /// The value inside the returned context will be `None` if the account does not exist.
    pub async fn order_with_config(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<store_accounts::Order>>> {
        Ok(self
            .account_with_config::<ZeroCopy<store_accounts::Order>>(address, config)
            .await?
            .map(|a| a.map(|a| a.0)))
    }

    fn create_action_filters(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> (StoreFilter, Vec<RpcFilterType>) {
        let mut filters = Vec::default();
        if let Some(owner) = owner {
            filters.push(RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                bytemuck::offset_of!(store_types::ActionHeader, owner) + DISC_OFFSET,
                owner.as_ref(),
            )));
        }
        if let Some(market_token) = market_token {
            let market = self.find_market_address(store, market_token);
            filters.push(RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
                bytemuck::offset_of!(store_types::ActionHeader, market) + DISC_OFFSET,
                market.as_ref(),
            )));
        }
        let store_filter = StoreFilter::new(
            store,
            bytemuck::offset_of!(store_types::ActionHeader, store),
        );

        (store_filter, filters)
    }

    /// Fetch [`Order`](store_accounts::Order) accounts.
    pub async fn orders(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Order>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::Order>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, order)| (addr, order.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`Depsoit`](store_accounts::Deposit) account with its address.
    pub async fn deposit(&self, address: &Pubkey) -> crate::Result<store_accounts::Deposit> {
        Ok(self
            .account::<ZeroCopy<_>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch [`Deposit`](store_accounts::Deposit) accounts.
    pub async fn deposits(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Deposit>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::Deposit>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`Withdrawal`](store_accounts::Withdrawal) account with its address.
    pub async fn withdrawal(&self, address: &Pubkey) -> crate::Result<store_accounts::Withdrawal> {
        Ok(self
            .account::<ZeroCopy<store_accounts::Withdrawal>>(address)
            .await?
            .ok_or(crate::Error::NotFound)?
            .0)
    }

    /// Fetch [`Withdrawal`](store_accounts::Withdrawal) accounts.
    pub async fn withdrawals(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Withdrawal>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::Withdrawal>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`Shift`](store_accounts::Shift) accounts.
    pub async fn shifts(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::Shift>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::Shift>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`GlvDeposit`](store_accounts::GlvDeposit) accounts.
    pub async fn glv_deposits(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::GlvDeposit>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::GlvDeposit>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`GlvWithdrawal`](store_accounts::GlvWithdrawal) accounts.
    pub async fn glv_withdrawals(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::GlvWithdrawal>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::GlvWithdrawal>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`GlvShift`](store_accounts::GlvShift) accounts.
    pub async fn glv_shifts(
        &self,
        store: &Pubkey,
        owner: Option<&Pubkey>,
        market_token: Option<&Pubkey>,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::GlvShift>> {
        let (store_filter, filters) = self.create_action_filters(store, owner, market_token);

        let orders = self
            .store_accounts::<ZeroCopy<store_accounts::GlvShift>>(Some(store_filter), filters)
            .await?
            .into_iter()
            .map(|(addr, action)| (addr, action.0))
            .collect();

        Ok(orders)
    }

    /// Fetch [`PriceFeed`](store_accounts::PriceFeed) account with its address.
    pub async fn price_feed(
        &self,
        address: &Pubkey,
    ) -> crate::Result<Option<store_accounts::PriceFeed>> {
        Ok(self
            .account::<ZeroCopy<store_accounts::PriceFeed>>(address)
            .await?
            .map(|a| a.0))
    }

    /// Get the [`PubsubClient`].
    pub async fn pub_sub(&self) -> crate::Result<&PubsubClient> {
        let client = self
            .pub_sub
            .get_or_try_init(|| {
                PubsubClient::new(self.cluster().clone(), self.subscription_config.clone())
            })
            .await?;
        Ok(client)
    }

    /// Subscribe to [`GMSOLCPIEvent`]s from the store program.
    #[cfg(feature = "decode")]
    pub async fn subscribe_store_cpi_events(
        &self,
        commitment: Option<CommitmentConfig>,
    ) -> crate::Result<impl futures_util::Stream<Item = crate::Result<WithSlot<Vec<GMSOLCPIEvent>>>>>
    {
        use futures_util::TryStreamExt;
        use transaction_history::extract_cpi_events;

        let program_id = self.store_program_id();
        let event_authority = self.store_event_authority();
        let query = Arc::new(self.store_program().rpc());
        let commitment = commitment.unwrap_or(self.subscription_config.commitment);
        let signatures = self
            .pub_sub()
            .await?
            .logs_subscribe(&event_authority, Some(commitment))
            .await?
            .and_then(|txn| {
                let signature = txn
                    .map(|txn| txn.signature.parse().map_err(crate::Error::custom))
                    .transpose();
                async move { signature }
            });
        let events = extract_cpi_events(
            signatures,
            query,
            program_id,
            &event_authority,
            commitment,
            Some(0),
        )
        .try_filter_map(|event| {
            let decoded = event
                .map(|event| {
                    event
                        .events
                        .iter()
                        .map(|event| GMSOLCPIEvent::decode(event).map_err(crate::Error::from))
                        .collect::<crate::Result<Vec<_>>>()
                })
                .transpose()
                .inspect_err(|err| tracing::error!(%err, "decode error"))
                .ok();
            async move { Ok(decoded) }
        });
        Ok(events)
    }

    /// Fetch historical [`GMSOLCPIEvent`]s for the given account.
    #[cfg(feature = "decode")]
    pub async fn historical_store_cpi_events(
        &self,
        address: &Pubkey,
        commitment: Option<CommitmentConfig>,
    ) -> crate::Result<impl futures_util::Stream<Item = crate::Result<WithSlot<Vec<GMSOLCPIEvent>>>>>
    {
        use futures_util::TryStreamExt;
        use transaction_history::{extract_cpi_events, fetch_transaction_history_with_config};

        let commitment = commitment.unwrap_or(self.commitment());
        let client = Arc::new(self.store_program().rpc());
        let signatures = fetch_transaction_history_with_config(
            client.clone(),
            address,
            commitment,
            None,
            None,
            None,
        )
        .await?;
        let events = extract_cpi_events(
            signatures,
            client,
            self.store_program_id(),
            &self.store_event_authority(),
            commitment,
            Some(0),
        )
        .try_filter(|events| std::future::ready(!events.value().events.is_empty()))
        .and_then(|encoded| {
            let decoded = encoded
                .map(|event| {
                    event
                        .events
                        .iter()
                        .map(|event| GMSOLCPIEvent::decode(event).map_err(crate::Error::from))
                        .collect::<crate::Result<Vec<_>>>()
                })
                .transpose();
            async move { decoded }
        });
        Ok(events)
    }

    /// Wait for an order to be completed using current slot as min context slot.
    #[cfg(feature = "decode")]
    pub async fn complete_order(
        &self,
        address: &Pubkey,
        commitment: Option<CommitmentConfig>,
    ) -> crate::Result<Option<store_events::TradeEvent>> {
        let slot = self.get_slot(None).await?;
        self.complete_order_with_config(
            address,
            slot,
            std::time::Duration::from_secs(5),
            commitment,
        )
        .await
    }

    /// Get last order events.
    #[cfg(feature = "decode")]
    pub async fn last_order_events(
        &self,
        order: &Pubkey,
        before_slot: u64,
        commitment: CommitmentConfig,
    ) -> crate::Result<Vec<GMSOLCPIEvent>> {
        use futures_util::{StreamExt, TryStreamExt};

        let events = self
            .historical_store_cpi_events(order, Some(commitment))
            .await?
            .try_filter(|events| {
                let pass = events.slot() <= before_slot;
                async move { pass }
            })
            .take(1);
        futures_util::pin_mut!(events);
        match events.next().await.transpose()? {
            Some(events) => Ok(events.into_value()),
            None => Err(crate::Error::custom(format!(
                "events not found, slot={before_slot}"
            ))),
        }
    }

    /// Wait for an order to be completed with the given config.
    #[cfg(feature = "decode")]
    pub async fn complete_order_with_config(
        &self,
        address: &Pubkey,
        mut slot: u64,
        polling: std::time::Duration,
        commitment: Option<CommitmentConfig>,
    ) -> crate::Result<Option<store_events::TradeEvent>> {
        use futures_util::{StreamExt, TryStreamExt};
        use solana_account_decoder::UiAccountEncoding;

        let mut trade = None;
        let commitment = commitment.unwrap_or(self.subscription_config.commitment);

        let events = self.subscribe_store_cpi_events(Some(commitment)).await?;

        let config = RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            commitment: Some(commitment),
            min_context_slot: Some(slot),
            ..Default::default()
        };
        let mut slot_reached = self.get_slot(Some(commitment)).await? >= slot;
        if slot_reached {
            let order = self.order_with_config(address, config.clone()).await?;
            slot = order.slot();
            let order = order.into_value();
            if order.is_none() {
                let events = self.last_order_events(address, slot, commitment).await?;
                return Ok(events
                    .into_iter()
                    .filter_map(|event| {
                        if let GMSOLCPIEvent::TradeEvent(event) = event {
                            Some(event)
                        } else {
                            None
                        }
                    })
                    .next());
            }
        }
        let address = *address;
        let stream = events
            .try_filter_map(|events| async {
                if events.slot() < slot {
                    return Ok(None);
                }
                let events = events
                    .into_value()
                    .into_iter()
                    .filter(|event| {
                        matches!(
                            event,
                            GMSOLCPIEvent::TradeEvent(_) | GMSOLCPIEvent::OrderRemoved(_)
                        )
                    })
                    .map(Ok);
                Ok(Some(futures_util::stream::iter(events)))
            })
            .try_flatten();
        let stream =
            tokio_stream::StreamExt::timeout_repeating(stream, tokio::time::interval(polling));
        futures_util::pin_mut!(stream);
        while let Some(res) = stream.next().await {
            match res {
                Ok(Ok(event)) => match event {
                    GMSOLCPIEvent::TradeEvent(event) => {
                        trade = Some(event);
                    }
                    GMSOLCPIEvent::OrderRemoved(_remove) => {
                        return Ok(trade);
                    }
                    _ => unreachable!(),
                },
                Ok(Err(err)) => {
                    return Err(err);
                }
                Err(_elapsed) => {
                    if slot_reached {
                        let res = self.order_with_config(&address, config.clone()).await?;
                        if res.value().is_none() {
                            let events = self
                                .last_order_events(&address, res.slot(), commitment)
                                .await?;
                            return Ok(events
                                .into_iter()
                                .filter_map(|event| {
                                    if let GMSOLCPIEvent::TradeEvent(event) = event {
                                        Some(event)
                                    } else {
                                        None
                                    }
                                })
                                .next());
                        }
                    } else {
                        slot_reached = self.get_slot(Some(commitment)).await? >= slot;
                    }
                }
            }
        }
        Err(crate::Error::custom("the watch stream end"))
    }

    /// Shutdown the client gracefully.
    pub async fn shutdown(&self) -> crate::Result<()> {
        self.pub_sub().await?.shutdown().await
    }

    /// Get GT exchanges.
    pub async fn gt_exchanges(
        &self,
        store: &Pubkey,
        owner: &Pubkey,
    ) -> crate::Result<BTreeMap<Pubkey, store_accounts::GtExchange>> {
        use store_accounts::GtExchange;

        let store_filter = StoreFilter::new(store, bytemuck::offset_of!(GtExchange, store));
        let owner_filter = RpcFilterType::Memcmp(Memcmp::new_base58_encoded(
            8 + bytemuck::offset_of!(GtExchange, owner),
            owner.as_ref(),
        ));
        let exchanges = self
            .store_accounts::<ZeroCopy<GtExchange>>(Some(store_filter), Some(owner_filter))
            .await?;
        Ok(exchanges
            .into_iter()
            .map(|(address, exchange)| (address, exchange.0))
            .collect())
    }

    /// Fetch [`InstructionBuffer`] account with its address.
    pub async fn instruction_buffer(
        &self,
        address: &Pubkey,
    ) -> crate::Result<Option<InstructionBuffer>> {
        self.account::<InstructionBuffer>(address).await
    }
}

/// Store Filter.
#[derive(Debug)]
pub struct StoreFilter {
    /// Store.
    store: Pubkey,
    /// Store offset.
    store_offset: usize,
    /// Ignore disc bytes.
    ignore_disc_offset: bool,
}

impl StoreFilter {
    /// Create a new store filter.
    pub fn new(store: &Pubkey, store_offset: usize) -> Self {
        Self {
            store: *store,
            store_offset,
            ignore_disc_offset: false,
        }
    }

    /// Ignore discriminator offset.
    pub fn ignore_disc_offset(mut self, ignore: bool) -> Self {
        self.ignore_disc_offset = ignore;
        self
    }

    /// Store offset.
    pub fn store_offset(&self) -> usize {
        if self.ignore_disc_offset {
            self.store_offset
        } else {
            self.store_offset + DISC_OFFSET
        }
    }
}

impl From<StoreFilter> for RpcFilterType {
    fn from(filter: StoreFilter) -> Self {
        let store = filter.store;
        let store_offset = filter.store_offset();
        RpcFilterType::Memcmp(Memcmp::new_base58_encoded(store_offset, store.as_ref()))
    }
}
