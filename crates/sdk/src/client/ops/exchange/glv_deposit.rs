use std::{
    collections::{BTreeSet, HashMap},
    ops::Deref,
};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::{
    accounts::{Glv, GlvDeposit},
    client::{accounts, args},
    types::CreateGlvDepositParams,
    ID,
};
use gmsol_solana_utils::{
    bundle_builder::{BundleBuilder, BundleOptions},
    compute_budget::ComputeBudget,
    make_bundle_builder::{MakeBundleBuilder, SetExecutionFee},
    transaction_builder::TransactionBuilder,
};
use gmsol_utils::{
    action::ActionFlag,
    market::{HasMarketMeta, MarketMeta},
    oracle::PriceProviderKind,
    swap::SwapActionParams,
    token_config::{TokenMapAccess, TokensWithFeed},
};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount, instruction::AccountMeta, pubkey::Pubkey,
    signer::Signer, system_program,
};

use crate::{
    builders::utils::{generate_nonce, get_ata_or_owner_with_program_id},
    client::{
        feeds_parser::{FeedAddressMap, FeedsParser},
        ops::{glv::split_to_accounts, token_account::TokenAccountOps},
        pull_oracle::{FeedIds, PullOraclePriceConsumer},
    },
    pda::NonceBytes,
    utils::{optional::fix_optional_account_metas, zero_copy::ZeroCopy},
};

use super::ExchangeOps;

/// Compute unit limit for `execute_glv_deposit`.
pub const EXECUTE_GLV_DEPOSIT_COMPUTE_BUDGET: u32 = 800_000;

/// Min execution lamports for GLV deposit.
pub const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

/// Create GLV deposit builder.
pub struct CreateGlvDepositBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    glv_token: Pubkey,
    market_token: Pubkey,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    long_token_swap_path: Vec<Pubkey>,
    short_token_swap_path: Vec<Pubkey>,
    market_token_amount: u64,
    initial_long_token_amount: u64,
    initial_short_token_amount: u64,
    min_market_token_amount: u64,
    min_glv_token_amount: u64,
    max_execution_lamports: u64,
    receiver: Option<Pubkey>,
    nonce: Option<NonceBytes>,
    market_token_source: Option<Pubkey>,
    initial_long_token_source: Option<Pubkey>,
    initial_short_token_source: Option<Pubkey>,
    hint: Option<CreateGlvDepositHint>,
    should_unwrap_native_token: bool,
}

/// Hint for [`CreateGlvDepositBuilder`].
#[derive(Clone)]
pub struct CreateGlvDepositHint {
    long_token_mint: Pubkey,
    short_token_mint: Pubkey,
}

impl CreateGlvDepositHint {
    /// Create from market meta.
    pub fn new(meta: &impl HasMarketMeta) -> Self {
        let meta = meta.market_meta();
        Self {
            long_token_mint: meta.long_token_mint,
            short_token_mint: meta.short_token_mint,
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CreateGlvDepositBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: Pubkey,
        glv_token: Pubkey,
        market_token: Pubkey,
    ) -> Self {
        Self {
            client,
            store,
            glv_token,
            market_token,
            initial_long_token: None,
            initial_short_token: None,
            long_token_swap_path: vec![],
            short_token_swap_path: vec![],
            market_token_amount: 0,
            initial_long_token_amount: 0,
            initial_short_token_amount: 0,
            min_market_token_amount: 0,
            min_glv_token_amount: 0,
            max_execution_lamports: MIN_EXECUTION_LAMPORTS,
            receiver: None,
            nonce: None,
            market_token_source: None,
            initial_long_token_source: None,
            initial_short_token_source: None,
            hint: None,
            should_unwrap_native_token: true,
        }
    }

    /// Set the nonce.
    pub fn nonce(&mut self, nonce: NonceBytes) -> &mut Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set max execution fee allowed to use.
    pub fn max_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.max_execution_lamports = lamports;
        self
    }

    /// Set long swap path.
    pub fn long_token_swap_path(&mut self, market_tokens: Vec<Pubkey>) -> &mut Self {
        self.long_token_swap_path = market_tokens;
        self
    }

    /// Set short swap path.
    pub fn short_token_swap_path(&mut self, market_tokens: Vec<Pubkey>) -> &mut Self {
        self.short_token_swap_path = market_tokens;
        self
    }

    /// Set the market token amount and source to deposit with.
    pub fn market_token_deposit(
        &mut self,
        amount: u64,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.market_token_amount = amount;
        self.market_token_source = token_account.copied();
        self
    }

    /// Set the initial long token amount and source to deposit with.
    pub fn long_token_deposit(
        &mut self,
        amount: u64,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.initial_long_token = token.copied();
        self.initial_long_token_amount = amount;
        self.initial_long_token_source = token_account.copied();
        self
    }

    /// Set the initial short tokens and source to deposit with.
    pub fn short_token_deposit(
        &mut self,
        amount: u64,
        token: Option<&Pubkey>,
        token_account: Option<&Pubkey>,
    ) -> &mut Self {
        self.initial_short_token = token.cloned();
        self.initial_short_token_amount = amount;
        self.initial_short_token_source = token_account.copied();
        self
    }

    /// Set min GLV token amount.
    pub fn min_glv_token_amount(&mut self, amount: u64) -> &mut Self {
        self.min_glv_token_amount = amount;
        self
    }

    /// Set min market token amount.
    pub fn min_market_token_amount(&mut self, amount: u64) -> &mut Self {
        self.min_market_token_amount = amount;
        self
    }

    /// Set whether to unwrap native token.
    /// Defaults to should unwrap.
    pub fn should_unwrap_native_token(&mut self, should_unwrap: bool) -> &mut Self {
        self.should_unwrap_native_token = should_unwrap;
        self
    }

    /// Set Receiver.
    /// Defaults to the payer.
    pub fn receiver(&mut self, receiver: Option<Pubkey>) -> &mut Self {
        self.receiver = receiver;
        self
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CreateGlvDepositHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    fn market_address(&self) -> Pubkey {
        self.client
            .find_market_address(&self.store, &self.market_token)
    }

    async fn prepare_hint(&mut self) -> crate::Result<CreateGlvDepositHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let market = self.market_address();
                let market = self.client.market(&market).await?;
                let hint = CreateGlvDepositHint::new(&MarketMeta::from(market.meta));
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Build.
    pub async fn build_with_address(
        &mut self,
    ) -> crate::Result<(TransactionBuilder<'a, C>, Pubkey)> {
        let hint = self.prepare_hint().await?;

        let nonce = self.nonce.unwrap_or_else(|| generate_nonce().to_bytes());
        let owner = self.client.payer();
        let receiver = self.receiver.unwrap_or(owner);
        let glv_deposit = self
            .client
            .find_glv_deposit_address(&self.store, &owner, &nonce);
        let market = self.market_address();
        let glv = self.client.find_glv_address(&self.glv_token);
        let token_program_id = anchor_spl::token::ID;
        let glv_token_program_id = anchor_spl::token_2022::ID;

        let mut initial_long_token = None;
        let mut initial_short_token = None;

        let glv_token_escrow = get_associated_token_address_with_program_id(
            &glv_deposit,
            &self.glv_token,
            &glv_token_program_id,
        );
        let market_token_escrow = get_associated_token_address_with_program_id(
            &glv_deposit,
            &self.market_token,
            &token_program_id,
        );
        let mut initial_long_token_escrow = None;
        let mut initial_short_token_escrow = None;

        let mut market_token_source = None;
        let mut initial_long_token_source = None;
        let mut initial_short_token_source = None;

        // Prepare the ATA for receiving GLV tokens.
        let mut prepare = self.client.prepare_associated_token_account(
            &self.glv_token,
            &glv_token_program_id,
            Some(&receiver),
        );

        // Prepare the escrow account for GLV tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &self.glv_token,
            &glv_token_program_id,
            Some(&glv_deposit),
        ));

        // Prepare the escrow account for market tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &self.market_token,
            &token_program_id,
            Some(&glv_deposit),
        ));

        if self.market_token_amount != 0 {
            market_token_source = Some(self.market_token_source.unwrap_or_else(|| {
                get_associated_token_address_with_program_id(
                    &owner,
                    &self.market_token,
                    &token_program_id,
                )
            }));
        }

        if self.initial_long_token_amount != 0 {
            let token = self.initial_long_token.unwrap_or(hint.long_token_mint);
            initial_long_token = Some(token);
            initial_long_token_source = Some(self.initial_long_token_source.unwrap_or_else(|| {
                get_associated_token_address_with_program_id(&owner, &token, &token_program_id)
            }));
            initial_long_token_escrow = Some(get_associated_token_address_with_program_id(
                &glv_deposit,
                &token,
                &token_program_id,
            ));

            // Prepare the escrow account.
            prepare = prepare.merge(self.client.prepare_associated_token_account(
                &token,
                &token_program_id,
                Some(&glv_deposit),
            ));
        }

        if self.initial_short_token_amount != 0 {
            let token = self.initial_short_token.unwrap_or(hint.short_token_mint);
            initial_short_token = Some(token);
            initial_short_token_source =
                Some(self.initial_short_token_source.unwrap_or_else(|| {
                    get_associated_token_address_with_program_id(&owner, &token, &token_program_id)
                }));
            initial_short_token_escrow = Some(get_associated_token_address_with_program_id(
                &glv_deposit,
                &token,
                &token_program_id,
            ));

            // Prepare the escrow account.
            prepare = prepare.merge(self.client.prepare_associated_token_account(
                &token,
                &token_program_id,
                Some(&glv_deposit),
            ));
        }

        let create = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CreateGlvDeposit {
                    owner,
                    receiver,
                    store: self.store,
                    market,
                    glv,
                    glv_deposit,
                    glv_token: self.glv_token,
                    market_token: self.market_token,
                    initial_long_token,
                    initial_short_token,
                    market_token_source,
                    initial_long_token_source,
                    initial_short_token_source,
                    glv_token_escrow,
                    market_token_escrow,
                    initial_long_token_escrow,
                    initial_short_token_escrow,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    glv_token_program: glv_token_program_id,
                    associated_token_program: anchor_spl::associated_token::ID,
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::CreateGlvDeposit {
                nonce,
                params: CreateGlvDepositParams {
                    execution_lamports: self.max_execution_lamports,
                    long_token_swap_length: self
                        .long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::unknown("swap path too long"))?,
                    short_token_swap_length: self
                        .short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::unknown("swap path too long"))?,
                    initial_long_token_amount: self.initial_long_token_amount,
                    initial_short_token_amount: self.initial_short_token_amount,
                    market_token_amount: self.market_token_amount,
                    min_market_token_amount: self.min_market_token_amount,
                    min_glv_token_amount: self.min_glv_token_amount,
                    should_unwrap_native_token: self.should_unwrap_native_token,
                },
            })
            .anchor_accounts(
                self.long_token_swap_path
                    .iter()
                    .chain(self.short_token_swap_path.iter())
                    .map(|token| AccountMeta {
                        pubkey: self.client.find_market_address(&self.store, token),
                        is_signer: false,
                        is_writable: false,
                    })
                    .collect::<Vec<_>>(),
            );

        Ok((prepare.merge(create), glv_deposit))
    }
}

/// Close GLV deposit builder.
pub struct CloseGlvDepositBuilder<'a, C> {
    client: &'a crate::Client<C>,
    glv_deposit: Pubkey,
    reason: String,
    hint: Option<CloseGlvDepositHint>,
}

/// Hint for [`CloseGlvDepositBuilder`].
#[derive(Clone)]
pub struct CloseGlvDepositHint {
    store: Pubkey,
    owner: Pubkey,
    receiver: Pubkey,
    glv_token: Pubkey,
    market_token: Pubkey,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    market_token_escrow: Pubkey,
    initial_long_token_escrow: Option<Pubkey>,
    initial_short_token_escrow: Option<Pubkey>,
    glv_token_escrow: Pubkey,
    should_unwrap_native_token: bool,
}

impl CloseGlvDepositHint {
    /// Create from the GLV deposit.
    pub fn new(glv_deposit: &GlvDeposit) -> Self {
        Self {
            store: glv_deposit.header.store,
            owner: glv_deposit.header.owner,
            receiver: glv_deposit.header.receiver,
            glv_token: glv_deposit.tokens.glv_token.token,
            market_token: glv_deposit.tokens.market_token.token,
            initial_long_token: glv_deposit.tokens.initial_long_token.token(),
            initial_short_token: glv_deposit.tokens.initial_short_token.token(),
            market_token_escrow: glv_deposit.tokens.market_token.account,
            initial_long_token_escrow: glv_deposit.tokens.initial_long_token.account(),
            initial_short_token_escrow: glv_deposit.tokens.initial_short_token.account(),
            glv_token_escrow: glv_deposit.tokens.glv_token.account,
            should_unwrap_native_token: glv_deposit
                .header
                .flags
                .get_flag(ActionFlag::ShouldUnwrapNativeToken),
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CloseGlvDepositBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, glv_deposit: Pubkey) -> Self {
        Self {
            client,
            glv_deposit,
            reason: "cancelled".to_string(),
            hint: None,
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseGlvDepositHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<CloseGlvDepositHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let glv_deposit = self
                    .client
                    .account::<ZeroCopy<GlvDeposit>>(&self.glv_deposit)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let hint = CloseGlvDepositHint::new(&glv_deposit);
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Build.
    pub async fn build(&mut self) -> crate::Result<TransactionBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;

        let token_program_id = anchor_spl::token::ID;
        let glv_token_program_id = anchor_spl::token_2022::ID;

        let payer = self.client.payer();

        let market_token_ata = get_associated_token_address_with_program_id(
            &hint.owner,
            &hint.market_token,
            &token_program_id,
        );
        let glv_token_ata = get_associated_token_address_with_program_id(
            &hint.receiver,
            &hint.glv_token,
            &glv_token_program_id,
        );
        let initial_long_token_ata = hint.initial_long_token.as_ref().map(|token| {
            get_ata_or_owner_with_program_id(
                &hint.owner,
                token,
                hint.should_unwrap_native_token,
                &token_program_id,
            )
        });
        let initial_short_token_ata = hint.initial_short_token.as_ref().map(|token| {
            get_ata_or_owner_with_program_id(
                &hint.owner,
                token,
                hint.should_unwrap_native_token,
                &token_program_id,
            )
        });

        let rpc = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::CloseGlvDeposit {
                    executor: payer,
                    store: hint.store,
                    store_wallet: self.client.find_store_wallet_address(&hint.store),
                    owner: hint.owner,
                    receiver: hint.receiver,
                    market_token: hint.market_token,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    glv_token: hint.glv_token,
                    glv_deposit: self.glv_deposit,
                    market_token_escrow: hint.market_token_escrow,
                    initial_long_token_escrow: hint.initial_long_token_escrow,
                    initial_short_token_escrow: hint.initial_short_token_escrow,
                    glv_token_escrow: hint.glv_token_escrow,
                    market_token_ata,
                    initial_long_token_ata,
                    initial_short_token_ata,
                    glv_token_ata,
                    system_program: system_program::ID,
                    token_program: token_program_id,
                    glv_token_program: glv_token_program_id,
                    associated_token_program: anchor_spl::associated_token::ID,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::CloseGlvDeposit {
                reason: self.reason.clone(),
            });

        Ok(rpc)
    }
}

/// Execute GLV deposit builder.
pub struct ExecuteGlvDepositBuilder<'a, C> {
    client: &'a crate::Client<C>,
    oracle: Pubkey,
    glv_deposit: Pubkey,
    execution_lamports: u64,
    cancel_on_execution_error: bool,
    hint: Option<ExecuteGlvDepositHint>,
    token_map: Option<Pubkey>,
    feeds_parser: FeedsParser,
    close: bool,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for [`ExecuteGlvDepositBuilder`].
#[derive(Clone)]
pub struct ExecuteGlvDepositHint {
    store: Pubkey,
    token_map: Pubkey,
    owner: Pubkey,
    receiver: Pubkey,
    glv_token: Pubkey,
    glv_market_tokens: BTreeSet<Pubkey>,
    market_token: Pubkey,
    initial_long_token: Option<Pubkey>,
    initial_short_token: Option<Pubkey>,
    market_token_escrow: Pubkey,
    initial_long_token_escrow: Option<Pubkey>,
    initial_short_token_escrow: Option<Pubkey>,
    glv_token_escrow: Pubkey,
    swap: SwapActionParams,
    /// Feeds.
    pub feeds: TokensWithFeed,
    should_unwrap_native_token: bool,
}

impl ExecuteGlvDepositHint {
    /// Create from the GLV deposit.
    pub fn new(
        glv: &Glv,
        glv_deposit: &GlvDeposit,
        token_map_address: &Pubkey,
        token_map: &impl TokenMapAccess,
        index_tokens: impl IntoIterator<Item = Pubkey>,
    ) -> crate::Result<Self> {
        let CloseGlvDepositHint {
            store,
            owner,
            receiver,
            glv_token,
            market_token,
            initial_long_token,
            initial_short_token,
            market_token_escrow,
            initial_long_token_escrow,
            initial_short_token_escrow,
            glv_token_escrow,
            should_unwrap_native_token,
        } = CloseGlvDepositHint::new(glv_deposit);
        let glv_market_tokens = glv.market_tokens().collect();
        let swap = glv_deposit.swap.into();
        let mut collector = glv.tokens_collector(Some(&swap));
        for token in index_tokens {
            collector.insert_token(&token);
        }

        Ok(Self {
            store,
            token_map: *token_map_address,
            owner,
            receiver,
            glv_token,
            glv_market_tokens,
            market_token,
            initial_long_token,
            initial_short_token,
            market_token_escrow,
            initial_long_token_escrow,
            initial_short_token_escrow,
            glv_token_escrow,
            swap,
            feeds: collector
                .to_feeds(token_map)
                .map_err(crate::Error::unknown)?,
            should_unwrap_native_token,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteGlvDepositBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        oracle: Pubkey,
        glv_deposit: Pubkey,
        cancel_on_execution_error: bool,
    ) -> Self {
        Self {
            client,
            oracle,
            glv_deposit,
            execution_lamports: 0,
            cancel_on_execution_error,
            hint: None,
            token_map: None,
            feeds_parser: Default::default(),
            close: true,
            alts: Default::default(),
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: ExecuteGlvDepositHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set token map address.
    pub fn token_map(&mut self, address: &Pubkey) -> &mut Self {
        self.token_map = Some(*address);
        self
    }

    /// Set whether to close the GLV deposit after execution.
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
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteGlvDepositHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let glv_deposit = self
                    .client
                    .account::<ZeroCopy<GlvDeposit>>(&self.glv_deposit)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;

                let glv_address = self
                    .client
                    .find_glv_address(&glv_deposit.tokens.glv_token.token);
                let glv = self
                    .client
                    .account::<ZeroCopy<Glv>>(&glv_address)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;

                let mut index_tokens = Vec::with_capacity(glv.num_markets());
                for token in glv.market_tokens() {
                    let market = self.client.find_market_address(&glv.store, &token);
                    let market = self.client.market(&market).await?;
                    index_tokens.push(market.meta.index_token_mint);
                }

                let store = &glv_deposit.header.store;
                let token_map_address = self
                    .client
                    .authorized_token_map_address(store)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let token_map = self.client.token_map(&token_map_address).await?;
                let hint = ExecuteGlvDepositHint::new(
                    &glv,
                    &glv_deposit,
                    &token_map_address,
                    &token_map,
                    index_tokens,
                )?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ExecuteGlvDepositBuilder<'a, C>
{
    async fn build_with_options(
        &mut self,
        options: BundleOptions,
    ) -> gmsol_solana_utils::Result<BundleBuilder<'a, C>> {
        let hint = self
            .prepare_hint()
            .await
            .map_err(gmsol_solana_utils::Error::custom)?;

        let token_program_id = anchor_spl::token::ID;
        let glv_token_program_id = anchor_spl::token_2022::ID;

        let authority = self.client.payer();
        let glv = self.client.find_glv_address(&hint.glv_token);
        let market = self
            .client
            .find_market_address(&hint.store, &hint.market_token);

        let initial_long_token_vault = hint
            .initial_long_token
            .as_ref()
            .map(|token| self.client.find_market_vault_address(&hint.store, token));
        let initial_short_token_vault = hint
            .initial_short_token
            .as_ref()
            .map(|token| self.client.find_market_vault_address(&hint.store, token));
        let market_token_vault = get_associated_token_address_with_program_id(
            &glv,
            &hint.market_token,
            &token_program_id,
        );

        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()
            .map_err(gmsol_solana_utils::Error::custom)?;
        let markets = hint
            .swap
            .unique_market_tokens_excluding_current(&hint.market_token)
            .map(|mint| AccountMeta {
                pubkey: self.client.find_market_address(&hint.store, mint),
                is_signer: false,
                is_writable: true,
            });

        let glv_accounts = split_to_accounts(
            hint.glv_market_tokens,
            &glv,
            &hint.store,
            self.client.store_program_id(),
            &token_program_id,
            false,
        )
        .0;

        let execute = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteGlvDeposit {
                    authority,
                    store: hint.store,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    glv,
                    market,
                    glv_deposit: self.glv_deposit,
                    glv_token: hint.glv_token,
                    market_token: hint.market_token,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    glv_token_escrow: hint.glv_token_escrow,
                    market_token_escrow: hint.market_token_escrow,
                    initial_long_token_escrow: hint.initial_long_token_escrow,
                    initial_short_token_escrow: hint.initial_short_token_escrow,
                    initial_long_token_vault,
                    initial_short_token_vault,
                    market_token_vault,
                    token_program: token_program_id,
                    glv_token_program: glv_token_program_id,
                    system_program: system_program::ID,
                    chainlink_program: None,
                    event_authority: self.client.store_event_authority(),
                    program: *self.client.store_program_id(),
                },
                &ID,
                self.client.store_program_id(),
            ))
            .anchor_args(args::ExecuteGlvDeposit {
                execution_lamports: self.execution_lamports,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(glv_accounts)
            .accounts(feeds.into_iter().chain(markets).collect::<Vec<_>>())
            .compute_budget(ComputeBudget::default().with_limit(EXECUTE_GLV_DEPOSIT_COMPUTE_BUDGET))
            .lookup_tables(self.alts.clone());

        let rpc = if self.close {
            let close = self
                .client
                .close_glv_deposit(&self.glv_deposit)
                .reason("executed")
                .hint(CloseGlvDepositHint {
                    store: hint.store,
                    owner: hint.owner,
                    receiver: hint.receiver,
                    glv_token: hint.glv_token,
                    market_token: hint.market_token,
                    initial_long_token: hint.initial_long_token,
                    initial_short_token: hint.initial_short_token,
                    market_token_escrow: hint.market_token_escrow,
                    initial_long_token_escrow: hint.initial_long_token_escrow,
                    initial_short_token_escrow: hint.initial_short_token_escrow,
                    glv_token_escrow: hint.glv_token_escrow,
                    should_unwrap_native_token: hint.should_unwrap_native_token,
                })
                .build()
                .await
                .map_err(gmsol_solana_utils::Error::custom)?;
            execute.merge(close)
        } else {
            execute
        };

        let mut tx = self.client.bundle_with_options(options);

        tx.try_push(rpc)?;

        Ok(tx)
    }
}

impl<C: Deref<Target = impl Signer> + Clone> PullOraclePriceConsumer
    for ExecuteGlvDepositBuilder<'_, C>
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

impl<C> SetExecutionFee for ExecuteGlvDepositBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_lamports = lamports;
        self
    }
}
