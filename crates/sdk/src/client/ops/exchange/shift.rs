use std::{collections::BTreeSet, ops::Deref};

use anchor_lang::prelude::AccountMeta;
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_programs::gmsol_store::{
    accounts::{Shift, Store},
    client::{accounts, args},
    types::CreateShiftParams,
    ID,
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    compute_budget::ComputeBudget,
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    market::{ordered_tokens, HasMarketMeta, MarketMeta},
    oracle::PriceProviderKind,
    pubkey::optional_address,
    token_config::{token_records, TokenMapAccess, TokensWithFeed},
};
use solana_sdk::{pubkey::Pubkey, signer::Signer, system_program};

use crate::{
    builders::utils::generate_nonce,
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        ops::token_account::TokenAccountOps,
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    pda::NonceBytes,
    utils::{optional::fix_optional_account_metas, zero_copy::ZeroCopy},
};

use super::{ExchangeOps, VirtualInventoryCollector};

/// Compute unit limit for `execute_shift`
pub const EXECUTE_SHIFT_COMPUTE_BUDGET: u32 = 400_000;

/// Min execution lamports for deposit.
pub const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

/// Create Shift Builder.
pub struct CreateShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    execution_fee: u64,
    amount: u64,
    min_to_market_token_amount: u64,
    nonce: Option<NonceBytes>,
    hint: CreateShiftHint,
    receiver: Pubkey,
}

/// Hint for creating shift.
#[derive(Default)]
pub struct CreateShiftHint {
    from_market_token_source: Option<Pubkey>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CreateShiftBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store: *store,
            from_market_token: *from_market_token,
            to_market_token: *to_market_token,
            execution_fee: MIN_EXECUTION_LAMPORTS,
            amount,
            min_to_market_token_amount: 0,
            nonce: None,
            hint: Default::default(),
            receiver: client.payer(),
        }
    }

    /// Set the nonce.
    pub fn nonce(&mut self, nonce: NonceBytes) -> &mut Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set exectuion fee allowed to use.
    pub fn execution_fee(&mut self, amount: u64) -> &mut Self {
        self.execution_fee = amount;
        self
    }

    /// Set min to market token amount.
    pub fn min_to_market_token_amount(&mut self, amount: u64) -> &mut Self {
        self.min_to_market_token_amount = amount;
        self
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CreateShiftHint) -> &mut Self {
        self.hint = hint;
        self
    }

    /// Set receiver.
    /// Defaults to the payer.
    pub fn receiver(&mut self, receiver: Pubkey) -> &mut Self {
        self.receiver = receiver;
        self
    }

    fn get_from_market_token_source(&self) -> Pubkey {
        match self.hint.from_market_token_source {
            Some(address) => address,
            None => get_associated_token_address(&self.client.payer(), &self.from_market_token),
        }
    }

    fn get_create_shift_params(&self) -> CreateShiftParams {
        CreateShiftParams {
            execution_lamports: self.execution_fee,
            from_market_token_amount: self.amount,
            min_to_market_token_amount: self.min_to_market_token_amount,
        }
    }

    /// Build a [`TransactionBuilder`] to create shift account and return the address of the shift account to create.
    pub fn build_with_address(&self) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let token_program_id = anchor_spl::token::ID;

        let owner = self.client.payer();
        let receiver = self.receiver;
        let nonce = self.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let shift = self.client.find_shift_address(&self.store, &owner, &nonce);

        let from_market = self
            .client
            .find_market_address(&self.store, &self.from_market_token);
        let to_market = self
            .client
            .find_market_address(&self.store, &self.to_market_token);

        let from_market_token_escrow =
            get_associated_token_address(&shift, &self.from_market_token);
        let to_market_token_escrow = get_associated_token_address(&shift, &self.to_market_token);
        let to_market_token_ata = get_associated_token_address(&receiver, &self.to_market_token);

        let prepare_escrow = self
            .client
            .prepare_associated_token_account(
                &self.from_market_token,
                &token_program_id,
                Some(&shift),
            )
            .merge(self.client.prepare_associated_token_account(
                &self.to_market_token,
                &token_program_id,
                Some(&shift),
            ));

        let prepare_ata = self.client.prepare_associated_token_account(
            &self.to_market_token,
            &token_program_id,
            Some(&receiver),
        );

        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CreateShift {
                owner,
                receiver,
                store: self.store,
                from_market,
                to_market,
                shift,
                from_market_token: self.from_market_token,
                to_market_token: self.to_market_token,
                from_market_token_escrow,
                to_market_token_escrow,
                from_market_token_source: self.get_from_market_token_source(),
                to_market_token_ata,
                system_program: system_program::ID,
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(args::CreateShift {
                nonce,
                params: self.get_create_shift_params(),
            });

        Ok((prepare_escrow.merge(prepare_ata).merge(rpc), shift))
    }
}

/// Close Shift Builder.
pub struct CloseShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    shift: Pubkey,
    reason: String,
    hint: Option<CloseShiftHint>,
}

/// Hint for `close_shift` instruction.
#[derive(Clone)]
pub struct CloseShiftHint {
    store: Pubkey,
    owner: Pubkey,
    receiver: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    from_market_token_escrow: Pubkey,
    to_market_token_escrow: Pubkey,
}

impl CloseShiftHint {
    /// Create hint for `close_shift` instruction.
    pub fn new(shift: &Shift) -> crate::Result<Self> {
        let tokens = &shift.tokens;
        Ok(Self {
            store: shift.header.store,
            owner: shift.header.owner,
            receiver: shift.header.receiver,
            from_market_token: tokens.from_market_token.token,
            from_market_token_escrow: tokens.from_market_token.account,
            to_market_token: tokens.to_market_token.token,
            to_market_token_escrow: tokens.to_market_token.account,
        })
    }

    #[allow(clippy::wrong_self_convention)]
    fn from_market_token_ata(&self) -> Pubkey {
        get_associated_token_address(&self.owner, &self.from_market_token)
    }

    fn to_market_token_ata(&self) -> Pubkey {
        get_associated_token_address(&self.receiver, &self.to_market_token)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CloseShiftBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, shift: &Pubkey) -> Self {
        Self {
            client,
            shift: *shift,
            hint: None,
            reason: String::from("cancelled"),
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseShiftHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    /// Prepare hint if needed
    pub async fn prepare_hint(&mut self) -> crate::Result<CloseShiftHint> {
        let hint = match &self.hint {
            Some(hint) => hint.clone(),
            None => {
                let shift = self
                    .client
                    .account::<ZeroCopy<_>>(&self.shift)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let hint = CloseShiftHint::new(&shift.0)?;
                self.hint = Some(hint.clone());
                hint
            }
        };

        Ok(hint)
    }

    /// Build a [`TransactionBuilder`] to close shift account.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let executor = self.client.payer();
        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CloseShift {
                executor,
                store: hint.store,
                store_wallet: self.client.find_store_wallet_address(&hint.store),
                owner: hint.owner,
                receiver: hint.receiver,
                shift: self.shift,
                from_market_token: hint.from_market_token,
                to_market_token: hint.to_market_token,
                from_market_token_escrow: hint.from_market_token_escrow,
                to_market_token_escrow: hint.to_market_token_escrow,
                from_market_token_ata: hint.from_market_token_ata(),
                to_market_token_ata: hint.to_market_token_ata(),
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                event_authority: self.client.store_event_authority(),
                program: *self.client.store_program_id(),
            })
            .anchor_args(args::CloseShift {
                reason: self.reason.clone(),
            });

        Ok(rpc)
    }
}

/// Execute Shift Instruction Builder.
pub struct ExecuteShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    shift: Pubkey,
    execution_fee: u64,
    cancel_on_execution_error: bool,
    oracle: Pubkey,
    hint: Option<ExecuteShiftHint>,
    close: bool,
    feeds_parser: FeedsParser,
}

/// Hint for `execute_shift` instruction.
#[derive(Clone)]
pub struct ExecuteShiftHint {
    store: Pubkey,
    token_map: Pubkey,
    owner: Pubkey,
    receiver: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    from_market_token_escrow: Pubkey,
    to_market_token_escrow: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
    virtual_inventories: BTreeSet<Pubkey>,
}

impl ExecuteShiftHint {
    /// Create hint for `execute_shift` instruction.
    pub fn new(
        shift: &Shift,
        store: &Store,
        map: &impl TokenMapAccess,
        from_market: &impl HasMarketMeta,
        to_market: &impl HasMarketMeta,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<Self> {
        let ordered_tokens = ordered_tokens(from_market, to_market);
        let token_records = token_records(map, &ordered_tokens).map_err(crate::Error::custom)?;
        let feeds =
            TokensWithFeed::try_from_records(token_records).map_err(crate::Error::custom)?;

        let CloseShiftHint {
            store: store_address,
            owner,
            receiver,
            from_market_token,
            to_market_token,
            from_market_token_escrow,
            to_market_token_escrow,
        } = CloseShiftHint::new(shift)?;

        Ok(Self {
            store: store_address,
            owner,
            receiver,
            from_market_token,
            from_market_token_escrow,
            to_market_token,
            to_market_token_escrow,
            token_map: *optional_address(&store.token_map).ok_or(crate::Error::NotFound)?,
            feeds,
            virtual_inventories,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteShiftBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        oracle: &Pubkey,
        shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> Self {
        Self {
            client,
            shift: *shift,
            hint: None,
            execution_fee: 0,
            cancel_on_execution_error,
            oracle: *oracle,
            close: true,
            feeds_parser: Default::default(),
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: ExecuteShiftHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set whether to cancel the shift account on execution error.
    pub fn cancel_on_execution_error(&mut self, cancel: bool) -> &mut Self {
        self.cancel_on_execution_error = cancel;
        self
    }

    /// Set whether to close shift account after execution.
    pub fn close(&mut self, close: bool) -> &mut Self {
        self.close = close;
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<ExecuteShiftHint> {
        let hint = match &self.hint {
            Some(hint) => hint.clone(),
            None => {
                let shift = self
                    .client
                    .account::<ZeroCopy<Shift>>(&self.shift)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let store_address = &shift.0.header.store;
                let store = self.client.store(store_address).await?;
                let token_map_address = optional_address(&store.token_map)
                    .ok_or(crate::Error::custom("token map is not set"))?;
                let token_map = self.client.token_map(token_map_address).await?;
                let from_market_token = shift.0.tokens.from_market_token.token;
                let to_market_token = shift.0.tokens.to_market_token.token;
                let from_market = self
                    .client
                    .find_market_address(store_address, &from_market_token);
                let from_market = MarketMeta::from(self.client.market(&from_market).await?.meta);
                let to_market = self
                    .client
                    .find_market_address(store_address, &to_market_token);
                let to_market = MarketMeta::from(self.client.market(&to_market).await?.meta);
                let virtual_inventories = VirtualInventoryCollector::default()
                    .insert_market_token(&from_market_token)
                    .insert_market_token(&to_market_token)
                    .collect(self.client, store_address)
                    .await?;
                let hint = ExecuteShiftHint::new(
                    &shift.0,
                    &store,
                    &token_map,
                    &from_market,
                    &to_market,
                    virtual_inventories,
                )?;
                self.hint = Some(hint.clone());
                hint
            }
        };

        Ok(hint)
    }

    /// Build a [`TransactionBuilder`] for `execute_shift` instruction.
    async fn build_rpc(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let authority = self.client.payer();

        let from_market = self
            .client
            .find_market_address(&hint.store, &hint.from_market_token);
        let to_market = self
            .client
            .find_market_address(&hint.store, &hint.to_market_token);
        let from_market_token_vault = self
            .client
            .find_market_vault_address(&hint.store, &hint.from_market_token);

        let feeds = self.feeds_parser.parse_and_sort_by_tokens(&hint.feeds)?;
        let virtual_inventories = hint
            .virtual_inventories
            .iter()
            .map(|pubkey| AccountMeta::new(*pubkey, false))
            .collect();

        let mut rpc = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteShift {
                    authority,
                    store: hint.store,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    from_market,
                    to_market,
                    shift: self.shift,
                    from_market_token: hint.from_market_token,
                    to_market_token: hint.to_market_token,
                    from_market_token_escrow: hint.from_market_token_escrow,
                    to_market_token_escrow: hint.to_market_token_escrow,
                    from_market_token_vault,
                    token_program: anchor_spl::token::ID,
                    chainlink_program: None,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::ExecuteShift {
                execution_lamports: self.execution_fee,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(feeds)
            .accounts(virtual_inventories)
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_SHIFT_COMPUTE_BUDGET));

        if self.close {
            let close = self
                .client
                .close_shift(&self.shift)
                .hint(CloseShiftHint {
                    store: hint.store,
                    owner: hint.owner,
                    receiver: hint.receiver,
                    from_market_token: hint.from_market_token,
                    to_market_token: hint.to_market_token,
                    from_market_token_escrow: hint.from_market_token_escrow,
                    to_market_token_escrow: hint.to_market_token_escrow,
                })
                .reason("executed")
                .build()
                .await?;
            rpc = rpc.merge(close);
        }

        Ok(rpc)
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ExecuteShiftBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let mut tx = self.client.bundle_with_options(options);
        tx.try_push(
            self.build_rpc()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?,
        )?;
        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ExecuteShiftBuilder<'_, C>
{
    async fn feed_ids(&mut self) -> crate::Result<FeedIds> {
        let hint = self.prepare_hint().await?;
        Ok(FeedIds::new(hint.store, hint.feeds))
    }

    fn process_feeds(
        &mut self,
        provider: PriceProviderKind,
        map: FeedAddressMap,
    ) -> crate::Result<()> {
        self.feeds_parser
            .insert_pull_oracle_feed_parser(provider, map);
        Ok(())
    }
}

impl<C> SetExecutionFee for ExecuteShiftBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}
