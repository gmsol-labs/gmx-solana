use std::{collections::HashMap, ops::Deref};

use anchor_spl::associated_token::get_associated_token_address;
use gmsol_programs::gmsol_store::{
    accounts::{GlvShift, Store},
    client::{accounts, args},
    types::CreateShiftParams,
    ID,
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    market::{ordered_tokens, HasMarketMeta, MarketMeta},
    oracle::PriceProviderKind,
    pubkey::optional_address,
    token_config::{token_records, TokenMapAccess, TokensWithFeed},
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey, signer::Signer, system_program,
};

use crate::{
    builders::utils::generate_nonce,
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    pda::NonceBytes,
    utils::{optional::fix_optional_account_metas, zero_copy::ZeroCopy},
};

use super::ExchangeOps;

/// Compute unit limit for `execute_glv_shift`.
pub const EXECUTE_GLV_WITHDRAWAL_COMPUTE_BUDGET: u32 = 800_000;

/// Min execution lamports for GLV shift.
pub const MIN_EXECUTION_LAMPORTS: u64 = 0;

/// Create Shift Builder.
pub struct CreateGlvShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    glv_token: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    execution_fee: u64,
    amount: u64,
    min_to_market_token_amount: u64,
    nonce: Option<NonceBytes>,
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CreateGlvShiftBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: &Pubkey,
        glv_token: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store: *store,
            glv_token: *glv_token,
            from_market_token: *from_market_token,
            to_market_token: *to_market_token,
            execution_fee: MIN_EXECUTION_LAMPORTS,
            amount,
            min_to_market_token_amount: 0,
            nonce: None,
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

    fn get_create_shift_params(&self) -> CreateShiftParams {
        CreateShiftParams {
            execution_lamports: self.execution_fee,
            from_market_token_amount: self.amount,
            min_to_market_token_amount: self.min_to_market_token_amount,
        }
    }

    /// Build a [`TransactionBuilder`] to create shift account and return the address of the shift account to create.
    pub fn build_with_address(&self) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let authority = self.client.payer();
        let nonce = self.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let glv = self.client.find_glv_address(&self.glv_token);
        let glv_shift = self
            .client
            .find_shift_address(&self.store, &authority, &nonce);

        let token_program_id = anchor_spl::token::ID;

        let from_market = self
            .client
            .find_market_address(&self.store, &self.from_market_token);
        let to_market = self
            .client
            .find_market_address(&self.store, &self.to_market_token);

        let from_market_token_vault = get_associated_token_address(&glv, &self.from_market_token);
        let to_market_token_vault = get_associated_token_address(&glv, &self.to_market_token);

        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CreateGlvShift {
                authority,
                store: self.store,
                from_market,
                to_market,
                glv_shift,
                from_market_token: self.from_market_token,
                to_market_token: self.to_market_token,
                from_market_token_vault,
                to_market_token_vault,
                system_program: system_program::ID,
                token_program: token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                glv,
            })
            .anchor_args(args::CreateGlvShift {
                nonce,
                params: self.get_create_shift_params(),
            });

        Ok((rpc, glv_shift))
    }
}

/// Close GLV Shift Builder.
pub struct CloseGlvShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    glv_shift: Pubkey,
    reason: String,
    hint: Option<CloseGlvShiftHint>,
}

/// Hint for `close_shift` instruction.
#[derive(Clone)]
pub struct CloseGlvShiftHint {
    store: Pubkey,
    owner: Pubkey,
    funder: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
}

impl CloseGlvShiftHint {
    /// Create hint for `close_shift` instruction.
    pub fn new(glv_shift: &GlvShift) -> crate::Result<Self> {
        let tokens = &glv_shift.shift.tokens;
        Ok(Self {
            store: glv_shift.shift.header.store,
            owner: glv_shift.shift.header.owner,
            funder: glv_shift.shift.header.rent_receiver,
            from_market_token: tokens.from_market_token.token,
            to_market_token: tokens.to_market_token.token,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CloseGlvShiftBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, shift: &Pubkey) -> Self {
        Self {
            client,
            glv_shift: *shift,
            hint: None,
            reason: String::from("cancelled"),
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseGlvShiftHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    /// Prepare hint if needed
    pub async fn prepare_hint(&mut self) -> crate::Result<CloseGlvShiftHint> {
        let hint = match &self.hint {
            Some(hint) => hint.clone(),
            None => {
                let shift = self
                    .client
                    .account::<ZeroCopy<_>>(&self.glv_shift)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let hint = CloseGlvShiftHint::new(&shift.0)?;
                self.hint = Some(hint.clone());
                hint
            }
        };

        Ok(hint)
    }

    /// Build a [`TransactionBuilder`] to close shift account.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let authority = self.client.payer();
        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CloseGlvShift {
                authority,
                funder: hint.funder,
                store: hint.store,
                store_wallet: self.client.find_store_wallet_address(&hint.store),
                glv: hint.owner,
                glv_shift: self.glv_shift,
                from_market_token: hint.from_market_token,
                to_market_token: hint.to_market_token,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                event_authority: self.client.store_event_authority(),
                program: *self.client.store_program_id(),
            })
            .anchor_args(args::CloseGlvShift {
                reason: self.reason.clone(),
            });

        Ok(rpc)
    }
}

/// Execute GLV Shift Instruction Builder.
pub struct ExecuteGlvShiftBuilder<'a, C> {
    client: &'a crate::Client<C>,
    shift: Pubkey,
    execution_fee: u64,
    cancel_on_execution_error: bool,
    oracle: Pubkey,
    hint: Option<ExecuteGlvShiftHint>,
    close: bool,
    feeds_parser: FeedsParser,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for `execute_shift` instruction.
#[derive(Clone)]
pub struct ExecuteGlvShiftHint {
    store: Pubkey,
    token_map: Pubkey,
    owner: Pubkey,
    funder: Pubkey,
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
}

impl ExecuteGlvShiftHint {
    /// Create hint for `execute_shift` instruction.
    pub fn new(
        glv_shift: &GlvShift,
        store: &Store,
        map: &impl TokenMapAccess,
        from_market: &impl HasMarketMeta,
        to_market: &impl HasMarketMeta,
    ) -> crate::Result<Self> {
        let CloseGlvShiftHint {
            store: store_address,
            owner,
            funder,
            from_market_token,
            to_market_token,
        } = CloseGlvShiftHint::new(glv_shift)?;

        let ordered_tokens = ordered_tokens(from_market, to_market);
        let token_records = token_records(map, &ordered_tokens).map_err(crate::Error::custom)?;
        let feeds =
            TokensWithFeed::try_from_records(token_records).map_err(crate::Error::custom)?;

        Ok(Self {
            store: store_address,
            owner,
            funder,
            from_market_token,
            to_market_token,
            token_map: *optional_address(&store.token_map).ok_or(crate::Error::NotFound)?,
            feeds,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteGlvShiftBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, oracle: &Pubkey, shift: &Pubkey) -> Self {
        Self {
            client,
            shift: *shift,
            hint: None,
            execution_fee: 0,
            cancel_on_execution_error: true,
            oracle: *oracle,
            close: true,
            feeds_parser: Default::default(),
            alts: Default::default(),
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: ExecuteGlvShiftHint) -> &mut Self {
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

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteGlvShiftHint> {
        let hint = match &self.hint {
            Some(hint) => hint.clone(),
            None => {
                let shift = self
                    .client
                    .account::<ZeroCopy<GlvShift>>(&self.shift)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let store_address = &shift.0.shift.header.store;
                let store = self.client.store(store_address).await?;
                let token_map_address = optional_address(&store.token_map)
                    .ok_or(crate::Error::custom("token map is not set"))?;
                let token_map = self.client.token_map(token_map_address).await?;
                let from_market_token = shift.0.shift.tokens.from_market_token.token;
                let to_market_token = shift.0.shift.tokens.to_market_token.token;
                let from_market = self
                    .client
                    .find_market_address(store_address, &from_market_token);
                let from_market = MarketMeta::from(self.client.market(&from_market).await?.meta);
                let to_market = self
                    .client
                    .find_market_address(store_address, &to_market_token);
                let to_market = MarketMeta::from(self.client.market(&to_market).await?.meta);
                let hint = ExecuteGlvShiftHint::new(
                    &shift.0,
                    &store,
                    &token_map,
                    &from_market,
                    &to_market,
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

        let glv = hint.owner;

        let from_market = self
            .client
            .find_market_address(&hint.store, &hint.from_market_token);
        let to_market = self
            .client
            .find_market_address(&hint.store, &hint.to_market_token);
        let from_market_token_vault = self
            .client
            .find_market_vault_address(&hint.store, &hint.from_market_token);

        let from_market_token_glv_vault =
            get_associated_token_address(&glv, &hint.from_market_token);
        let to_market_token_glv_vault = get_associated_token_address(&glv, &hint.to_market_token);

        let feeds = self.feeds_parser.parse_and_sort_by_tokens(&hint.feeds)?;

        let mut rpc = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteGlvShift {
                    authority,
                    store: hint.store,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    glv,
                    from_market,
                    to_market,
                    glv_shift: self.shift,
                    from_market_token: hint.from_market_token,
                    to_market_token: hint.to_market_token,
                    from_market_token_glv_vault,
                    to_market_token_glv_vault,
                    from_market_token_vault,
                    token_program: anchor_spl::token::ID,
                    chainlink_program: None,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::ExecuteGlvShift {
                execution_lamports: self.execution_fee,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(feeds)
            .lookup_tables(self.alts.clone());

        if self.close {
            let close = self
                .client
                .close_glv_shift(&self.shift)
                .hint(CloseGlvShiftHint {
                    store: hint.store,
                    owner: hint.owner,
                    funder: hint.funder,
                    from_market_token: hint.from_market_token,
                    to_market_token: hint.to_market_token,
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
    for ExecuteGlvShiftBuilder<'a, C>
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
    for ExecuteGlvShiftBuilder<'_, C>
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

impl<C> SetExecutionFee for ExecuteGlvShiftBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
        self
    }
}
