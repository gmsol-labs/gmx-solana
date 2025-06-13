use std::{
    collections::{BTreeSet, HashMap},
    ops::Deref,
};

use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_programs::gmsol_store::{
    accounts::{Glv, GlvWithdrawal},
    client::{accounts, args},
    types::CreateGlvWithdrawalParams,
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

use super::{ExchangeOps, VirtualInventoryCollector};

/// Compute unit limit for `execute_glv_withdrawal`.
pub const EXECUTE_GLV_WITHDRAWAL_COMPUTE_BUDGET: u32 = 800_000;

/// Min execution lamports for GLV withdrawal.
pub const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

/// Create GLV withdrawal builder.
pub struct CreateGlvWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    store: Pubkey,
    glv_token: Pubkey,
    market_token: Pubkey,
    final_long_token: Option<Pubkey>,
    final_short_token: Option<Pubkey>,
    long_token_swap_path: Vec<Pubkey>,
    short_token_swap_path: Vec<Pubkey>,
    glv_token_amount: u64,
    min_final_long_token_amount: u64,
    min_final_short_token_amount: u64,
    max_execution_lamports: u64,
    nonce: Option<NonceBytes>,
    glv_token_source: Option<Pubkey>,
    hint: Option<CreateGlvWithdrawalHint>,
    should_unwrap_native_token: bool,
    receiver: Pubkey,
}

/// Hint for [`CreateGlvWithdrawalBuilder`]
#[derive(Clone)]
pub struct CreateGlvWithdrawalHint {
    long_token_mint: Pubkey,
    short_token_mint: Pubkey,
}

impl CreateGlvWithdrawalHint {
    /// Create from market meta.
    pub fn new(meta: &impl HasMarketMeta) -> Self {
        let meta = meta.market_meta();
        Self {
            long_token_mint: meta.long_token_mint,
            short_token_mint: meta.short_token_mint,
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CreateGlvWithdrawalBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        store: Pubkey,
        glv_token: Pubkey,
        market_token: Pubkey,
        amount: u64,
    ) -> Self {
        Self {
            client,
            store,
            glv_token,
            market_token,
            final_long_token: None,
            final_short_token: None,
            long_token_swap_path: vec![],
            short_token_swap_path: vec![],
            glv_token_amount: amount,
            min_final_long_token_amount: 0,
            min_final_short_token_amount: 0,
            max_execution_lamports: MIN_EXECUTION_LAMPORTS,
            nonce: None,
            glv_token_source: None,
            hint: None,
            should_unwrap_native_token: true,
            receiver: client.payer(),
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

    /// Final long token config.
    pub fn final_long_token(
        &mut self,
        token: Option<&Pubkey>,
        min_amount: u64,
        swap_path: Vec<Pubkey>,
    ) -> &mut Self {
        self.final_long_token = token.copied();
        self.min_final_long_token_amount = min_amount;
        self.long_token_swap_path = swap_path;
        self
    }

    /// Final short token config.
    pub fn final_short_token(
        &mut self,
        token: Option<&Pubkey>,
        min_amount: u64,
        swap_path: Vec<Pubkey>,
    ) -> &mut Self {
        self.final_short_token = token.copied();
        self.min_final_short_token_amount = min_amount;
        self.short_token_swap_path = swap_path;
        self
    }

    /// Set GLV token source.
    pub fn glv_token_source(&mut self, address: &Pubkey) -> &mut Self {
        self.glv_token_source = Some(*address);
        self
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CreateGlvWithdrawalHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set whether to unwrap native token.
    /// Defaults to should unwrap.
    pub fn should_unwrap_native_token(&mut self, should_unwrap: bool) -> &mut Self {
        self.should_unwrap_native_token = should_unwrap;
        self
    }

    /// Set receiver.
    /// Defaults to the payer.
    pub fn receiver(&mut self, receiver: Option<Pubkey>) -> &mut Self {
        self.receiver = receiver.unwrap_or(self.client.payer());
        self
    }

    fn market_address(&self) -> Pubkey {
        self.client
            .find_market_address(&self.store, &self.market_token)
    }

    /// Prepare hint.
    pub async fn prepare_hint(&mut self) -> crate::Result<CreateGlvWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let market = self.market_address();
                let market = self.client.market(&market).await?;
                let meta = MarketMeta::from(market.meta);
                let hint = CreateGlvWithdrawalHint::new(&meta);
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
        let receiver = self.receiver;
        let glv_withdrawal = self
            .client
            .find_glv_withdrawal_address(&self.store, &owner, &nonce);
        let market = self.market_address();
        let glv = self.client.find_glv_address(&self.glv_token);
        let token_program_id = anchor_spl::token::ID;
        let glv_token_program_id = anchor_spl::token_2022::ID;

        let final_long_token = self.final_long_token.unwrap_or(hint.long_token_mint);
        let final_short_token = self.final_short_token.unwrap_or(hint.short_token_mint);

        let glv_token_source = self.glv_token_source.unwrap_or_else(|| {
            get_associated_token_address_with_program_id(
                &owner,
                &self.glv_token,
                &glv_token_program_id,
            )
        });

        let glv_token_escrow = get_associated_token_address_with_program_id(
            &glv_withdrawal,
            &self.glv_token,
            &glv_token_program_id,
        );
        let market_token_escrow = get_associated_token_address_with_program_id(
            &glv_withdrawal,
            &self.market_token,
            &token_program_id,
        );
        let final_long_token_escrow = get_associated_token_address_with_program_id(
            &glv_withdrawal,
            &final_long_token,
            &token_program_id,
        );
        let final_short_token_escrow = get_associated_token_address_with_program_id(
            &glv_withdrawal,
            &final_short_token,
            &token_program_id,
        );

        // Prepare the ATA for receiving final long tokens.
        let mut prepare = self.client.prepare_associated_token_account(
            &final_long_token,
            &token_program_id,
            Some(&receiver),
        );

        // Prepare the ATA for receiving final short tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &final_short_token,
            &token_program_id,
            Some(&receiver),
        ));

        // Prepare the escrow account for GLV tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &self.glv_token,
            &glv_token_program_id,
            Some(&glv_withdrawal),
        ));

        // Prepare the escrow account for market tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &self.market_token,
            &token_program_id,
            Some(&glv_withdrawal),
        ));

        // Prepare the escrow account for final long tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &final_long_token,
            &token_program_id,
            Some(&glv_withdrawal),
        ));
        // Prepare the escrow account for final long tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &final_short_token,
            &token_program_id,
            Some(&glv_withdrawal),
        ));

        let create = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CreateGlvWithdrawal {
                owner,
                receiver,
                store: self.store,
                market,
                glv,
                glv_withdrawal,
                glv_token: self.glv_token,
                market_token: self.market_token,
                final_long_token,
                final_short_token,
                glv_token_source,
                glv_token_escrow,
                market_token_escrow,
                final_long_token_escrow,
                final_short_token_escrow,
                system_program: system_program::ID,
                token_program: token_program_id,
                glv_token_program: glv_token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .anchor_args(args::CreateGlvWithdrawal {
                nonce,
                params: CreateGlvWithdrawalParams {
                    execution_lamports: self.max_execution_lamports,
                    long_token_swap_length: self
                        .long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::custom("swap path too long"))?,
                    short_token_swap_length: self
                        .short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::custom("swap path too long"))?,
                    glv_token_amount: self.glv_token_amount,
                    min_final_long_token_amount: self.min_final_long_token_amount,
                    min_final_short_token_amount: self.min_final_short_token_amount,
                    should_unwrap_native_token: self.should_unwrap_native_token,
                },
            })
            .accounts(
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

        Ok((prepare.merge(create), glv_withdrawal))
    }
}

/// Close GLV withdrawal builder.
pub struct CloseGlvWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    glv_withdrawal: Pubkey,
    reason: String,
    hint: Option<CloseGlvWithdrawalHint>,
}

/// Hint for [`CloseGlvWithdrawalBuilder`].
#[derive(Clone)]
pub struct CloseGlvWithdrawalHint {
    store: Pubkey,
    owner: Pubkey,
    receiver: Pubkey,
    glv_token: Pubkey,
    market_token: Pubkey,
    final_long_token: Pubkey,
    final_short_token: Pubkey,
    market_token_escrow: Pubkey,
    final_long_token_escrow: Pubkey,
    final_short_token_escrow: Pubkey,
    glv_token_escrow: Pubkey,
    should_unwrap_native_token: bool,
}

impl CloseGlvWithdrawalHint {
    /// Create from the GLV withdrawal.
    pub fn new(glv_withdrawal: &GlvWithdrawal) -> Self {
        Self {
            store: glv_withdrawal.header.store,
            owner: glv_withdrawal.header.owner,
            receiver: glv_withdrawal.header.receiver,
            glv_token: glv_withdrawal.tokens.glv_token.token,
            market_token: glv_withdrawal.tokens.market_token.token,
            final_long_token: glv_withdrawal.tokens.final_long_token.token,
            final_short_token: glv_withdrawal.tokens.final_short_token.token,
            market_token_escrow: glv_withdrawal.tokens.market_token.account,
            final_long_token_escrow: glv_withdrawal.tokens.final_long_token.account,
            final_short_token_escrow: glv_withdrawal.tokens.final_short_token.account,
            glv_token_escrow: glv_withdrawal.tokens.glv_token.account,
            should_unwrap_native_token: glv_withdrawal
                .header
                .flags
                .get_flag(ActionFlag::ShouldUnwrapNativeToken),
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> CloseGlvWithdrawalBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, glv_withdrawal: Pubkey) -> Self {
        Self {
            client,
            glv_withdrawal,
            reason: "cancelled".to_string(),
            hint: None,
        }
    }

    /// Set hint.
    pub fn hint(&mut self, hint: CloseGlvWithdrawalHint) -> &mut Self {
        self.hint = Some(hint);
        self
    }

    /// Set reason.
    pub fn reason(&mut self, reason: impl ToString) -> &mut Self {
        self.reason = reason.to_string();
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<CloseGlvWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let glv_deposit = self
                    .client
                    .account::<ZeroCopy<GlvWithdrawal>>(&self.glv_withdrawal)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;
                let hint = CloseGlvWithdrawalHint::new(&glv_deposit);
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
            &hint.owner,
            &hint.glv_token,
            &glv_token_program_id,
        );
        let final_long_token_ata = get_ata_or_owner_with_program_id(
            &hint.receiver,
            &hint.final_long_token,
            hint.should_unwrap_native_token,
            &token_program_id,
        );
        let final_short_token_ata = get_ata_or_owner_with_program_id(
            &hint.receiver,
            &hint.final_short_token,
            hint.should_unwrap_native_token,
            &token_program_id,
        );

        let rpc = self
            .client
            .store_transaction()
            .anchor_accounts(accounts::CloseGlvWithdrawal {
                executor: payer,
                store: hint.store,
                store_wallet: self.client.find_store_wallet_address(&hint.store),
                owner: hint.owner,
                receiver: hint.receiver,
                glv_withdrawal: self.glv_withdrawal,
                market_token: hint.market_token,
                final_long_token: hint.final_long_token,
                final_short_token: hint.final_short_token,
                glv_token: hint.glv_token,
                market_token_escrow: hint.market_token_escrow,
                final_long_token_escrow: hint.final_long_token_escrow,
                final_short_token_escrow: hint.final_short_token_escrow,
                market_token_ata,
                final_long_token_ata,
                final_short_token_ata,
                glv_token_escrow: hint.glv_token_escrow,
                glv_token_ata,
                system_program: system_program::ID,
                token_program: token_program_id,
                glv_token_program: glv_token_program_id,
                associated_token_program: anchor_spl::associated_token::ID,
                event_authority: self.client.store_event_authority(),
                program: *self.client.store_program_id(),
            })
            .anchor_args(args::CloseGlvWithdrawal {
                reason: self.reason.clone(),
            });
        Ok(rpc)
    }
}

/// Execute GLV withdrawal builder.
pub struct ExecuteGlvWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    oracle: Pubkey,
    glv_withdrawal: Pubkey,
    execution_lamports: u64,
    cancel_on_execution_error: bool,
    hint: Option<ExecuteGlvWithdrawalHint>,
    token_map: Option<Pubkey>,
    feeds_parser: FeedsParser,
    close: bool,
    alts: HashMap<Pubkey, Vec<Pubkey>>,
}

/// Hint for [`ExecuteGlvWithdrawalBuilder`].
#[derive(Clone)]
pub struct ExecuteGlvWithdrawalHint {
    close: CloseGlvWithdrawalHint,
    token_map: Pubkey,
    glv_market_tokens: BTreeSet<Pubkey>,
    swap: SwapActionParams,
    /// Feeds.
    pub feeds: TokensWithFeed,
    virtual_inventories: BTreeSet<Pubkey>,
}

impl Deref for ExecuteGlvWithdrawalHint {
    type Target = CloseGlvWithdrawalHint;

    fn deref(&self) -> &Self::Target {
        &self.close
    }
}

impl ExecuteGlvWithdrawalHint {
    /// Create a new hint.
    pub fn new(
        glv: &Glv,
        glv_withdrawal: &GlvWithdrawal,
        token_map_address: &Pubkey,
        token_map: &impl TokenMapAccess,
        index_tokens: impl IntoIterator<Item = Pubkey>,
        virtual_inventories: BTreeSet<Pubkey>,
    ) -> crate::Result<Self> {
        let glv_market_tokens = glv.market_tokens().collect();
        let swap = glv_withdrawal.swap.into();
        let mut collector = glv.tokens_collector(Some(&swap));
        for token in index_tokens {
            collector.insert_token(&token);
        }
        let close = CloseGlvWithdrawalHint::new(glv_withdrawal);
        Ok(Self {
            close,
            token_map: *token_map_address,
            glv_market_tokens,
            swap,
            feeds: collector
                .to_feeds(token_map)
                .map_err(crate::Error::custom)?,
            virtual_inventories,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteGlvWithdrawalBuilder<'a, C> {
    pub(super) fn new(
        client: &'a crate::Client<C>,
        oracle: Pubkey,
        glv_withdrawal: Pubkey,
        cancel_on_execution_error: bool,
    ) -> Self {
        Self {
            client,
            oracle,
            glv_withdrawal,
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
    pub fn hint(&mut self, hint: ExecuteGlvWithdrawalHint) -> &mut Self {
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
    pub async fn prepare_hint(&mut self) -> crate::Result<ExecuteGlvWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let glv_withdrawal = self
                    .client
                    .account::<ZeroCopy<GlvWithdrawal>>(&self.glv_withdrawal)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;

                let glv_address = self
                    .client
                    .find_glv_address(&glv_withdrawal.tokens.glv_token.token);
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

                let store = &glv_withdrawal.header.store;
                let token_map_address = self
                    .client
                    .authorized_token_map_address(store)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let token_map = self.client.token_map(&token_map_address).await?;
                let swap = glv_withdrawal.swap.into();
                let virtual_inventories = VirtualInventoryCollector::from_swap(&swap)
                    .collect(self.client, store)
                    .await?;
                let hint = ExecuteGlvWithdrawalHint::new(
                    &glv,
                    &glv_withdrawal,
                    &token_map_address,
                    &token_map,
                    index_tokens,
                    virtual_inventories,
                )?;
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> MakeBundleBuilder<'a, C>
    for ExecuteGlvWithdrawalBuilder<'a, C>
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
            .find_market_address(&hint.close.store, &hint.market_token);

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
        let virtual_inventories = hint
            .virtual_inventories
            .iter()
            .map(|pubkey| AccountMeta::new(*pubkey, false));

        let glv_accounts = split_to_accounts(
            hint.glv_market_tokens.iter().copied(),
            &glv,
            &hint.store,
            self.client.store_program_id(),
            &token_program_id,
            false,
        )
        .0;

        let final_long_token_vault = self
            .client
            .find_market_vault_address(&hint.store, &hint.final_long_token);
        let final_short_token_vault = self
            .client
            .find_market_vault_address(&hint.store, &hint.final_short_token);

        let market_token_vault = get_associated_token_address_with_program_id(
            &glv,
            &hint.market_token,
            &token_program_id,
        );

        let market_token_withdrawal_vault = self
            .client
            .find_market_vault_address(&hint.store, &hint.market_token);

        let execute = self
            .client
            .store_transaction()
            .accounts(fix_optional_account_metas(
                accounts::ExecuteGlvWithdrawal {
                    authority,
                    store: hint.store,
                    token_map: hint.token_map,
                    oracle: self.oracle,
                    glv,
                    market,
                    glv_withdrawal: self.glv_withdrawal,
                    glv_token: hint.glv_token,
                    market_token: hint.market_token,
                    final_long_token: hint.final_long_token,
                    final_short_token: hint.final_short_token,
                    glv_token_escrow: hint.glv_token_escrow,
                    market_token_escrow: hint.market_token_escrow,
                    final_long_token_escrow: hint.final_long_token_escrow,
                    final_short_token_escrow: hint.final_short_token_escrow,
                    market_token_withdrawal_vault,
                    final_long_token_vault,
                    final_short_token_vault,
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
            .anchor_args(args::ExecuteGlvWithdrawal {
                execution_lamports: self.execution_lamports,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(glv_accounts)
            .accounts(
                feeds
                    .into_iter()
                    .chain(markets)
                    .chain(virtual_inventories)
                    .collect::<Vec<_>>(),
            )
            .compute_budget(
                ComputeBudget::default().with_limit(EXECUTE_GLV_WITHDRAWAL_COMPUTE_BUDGET),
            )
            .lookup_tables(self.alts.clone());

        let rpc = if self.close {
            let close = self
                .client
                .close_glv_withdrawal(&self.glv_withdrawal)
                .reason("executed")
                .hint(hint.close)
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
    for ExecuteGlvWithdrawalBuilder<'_, C>
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

impl<C> SetExecutionFee for ExecuteGlvWithdrawalBuilder<'_, C> {
    fn set_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_lamports = lamports;
        self
    }
}
