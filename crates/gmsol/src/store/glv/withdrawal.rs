use std::{
    collections::{BTreeSet, HashMap},
    ops::Deref,
};

use anchor_client::{
    anchor_lang::{prelude::AccountMeta, system_program, Id},
    solana_sdk::{address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use gmsol_store::{
    accounts, instruction,
    ops::glv::CreateGlvWithdrawalParams,
    states::{
        common::{action::Action, swap::SwapParams, TokensWithFeed},
        glv::GlvWithdrawal,
        Glv, HasMarketMeta, NonceBytes, Pyth, TokenMapAccess,
    },
};

use crate::{
    exchange::generate_nonce,
    store::{token::TokenAccountOps, utils::FeedsParser},
    utils::{ComputeBudget, RpcBuilder, ZeroCopy},
};

use super::{split_to_accounts, GlvOps};

pub const EXECUTE_GLV_WITHDRAWAL_COMPUTE_BUDGET: u32 = 400_000;

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
            max_execution_lamports: GlvWithdrawal::MIN_EXECUTION_LAMPORTS,
            nonce: None,
            glv_token_source: None,
            hint: None,
        }
    }

    /// Set max execution fee allowed to use.
    pub fn max_execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.max_execution_lamports = lamports;
        self
    }

    /// Set long swap path.
    pub fn long_token_swap_path(
        &mut self,
        final_long_token: &Pubkey,
        market_tokens: Vec<Pubkey>,
    ) -> &mut Self {
        self.final_long_token = Some(*final_long_token);
        self.long_token_swap_path = market_tokens;
        self
    }

    /// Set short swap path.
    pub fn short_token_swap_path(
        &mut self,
        final_short_token: &Pubkey,
        market_tokens: Vec<Pubkey>,
    ) -> &mut Self {
        self.final_short_token = Some(*final_short_token);
        self.short_token_swap_path = market_tokens;
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

    fn market_address(&self) -> Pubkey {
        self.client
            .find_market_address(&self.store, &self.market_token)
    }

    async fn prepare_hint(&mut self) -> crate::Result<CreateGlvWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let market = self.market_address();
                let market = self.client.market(&market).await?;
                let hint = CreateGlvWithdrawalHint::new(&market);
                self.hint = Some(hint.clone());
                Ok(hint)
            }
        }
    }

    /// Build.
    pub async fn build_with_address(&mut self) -> crate::Result<(RpcBuilder<'a, C>, Pubkey)> {
        let hint = self.prepare_hint().await?;

        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        let owner = self.client.payer();
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
            None,
        );

        // Prepare the ATA for receiving final short tokens.
        prepare = prepare.merge(self.client.prepare_associated_token_account(
            &final_short_token,
            &token_program_id,
            None,
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
            .store_rpc()
            .accounts(accounts::CreateGlvWithdrawal {
                owner,
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
            .args(instruction::CreateGlvWithdrawal {
                nonce,
                params: CreateGlvWithdrawalParams {
                    execution_lamports: self.max_execution_lamports,
                    long_token_swap_length: self
                        .long_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::invalid_argument("swap path too long"))?,
                    short_token_swap_length: self
                        .short_token_swap_path
                        .len()
                        .try_into()
                        .map_err(|_| crate::Error::invalid_argument("swap path too long"))?,
                    glv_token_amount: self.glv_token_amount,
                    min_final_long_token_amount: self.min_final_long_token_amount,
                    min_final_short_token_amount: self.min_final_short_token_amount,
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

/// Hint for [`CloseGlvDepositBuilder`].
#[derive(Clone)]
pub struct CloseGlvWithdrawalHint {
    store: Pubkey,
    owner: Pubkey,
    glv_token: Pubkey,
    market_token: Pubkey,
    final_long_token: Pubkey,
    final_short_token: Pubkey,
    market_token_escrow: Pubkey,
    final_long_token_escrow: Pubkey,
    final_short_token_escrow: Pubkey,
    glv_token_escrow: Pubkey,
}

impl CloseGlvWithdrawalHint {
    /// Create from the GLV withdrawal.
    pub fn new(glv_withdrawal: &GlvWithdrawal) -> Self {
        Self {
            store: *glv_withdrawal.header().store(),
            owner: *glv_withdrawal.header().owner(),
            glv_token: glv_withdrawal.tokens().glv_token(),
            market_token: glv_withdrawal.tokens().market_token(),
            final_long_token: glv_withdrawal.tokens().final_long_token(),
            final_short_token: glv_withdrawal.tokens().final_short_token(),
            market_token_escrow: glv_withdrawal.tokens().market_token_account(),
            final_long_token_escrow: glv_withdrawal.tokens().final_long_token_account(),
            final_short_token_escrow: glv_withdrawal.tokens().final_short_token_account(),
            glv_token_escrow: glv_withdrawal.tokens().glv_token_account(),
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
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
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
        let final_long_token_ata = get_associated_token_address_with_program_id(
            &hint.owner,
            &hint.final_long_token,
            &token_program_id,
        );
        let final_short_token_ata = get_associated_token_address_with_program_id(
            &hint.owner,
            &hint.final_short_token,
            &token_program_id,
        );

        let rpc = self
            .client
            .store_rpc()
            .accounts(accounts::CloseGlvWithdrawal {
                executor: payer,
                store: hint.store,
                owner: hint.owner,
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
                program: self.client.store_program_id(),
            })
            .args(instruction::CloseGlvWithdrawal {
                reason: self.reason.clone(),
            });
        Ok(rpc)
    }
}

/// Execute GLV withdrawal builder.
pub struct ExecuteGlvWithdrawalBuilder<'a, C> {
    client: &'a crate::Client<C>,
    oracle: Pubkey,
    price_provider: Pubkey,
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
    swap: SwapParams,
    feeds: TokensWithFeed,
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
    ) -> crate::Result<Self> {
        let glv_market_tokens = glv.market_tokens().collect();
        let mut collector = glv.tokens_collector(Some(glv_withdrawal));
        for token in index_tokens {
            collector.insert_token(&token);
        }
        let close = CloseGlvWithdrawalHint::new(glv_withdrawal);
        Ok(Self {
            close,
            token_map: *token_map_address,
            glv_market_tokens,
            swap: *glv_withdrawal.swap(),
            feeds: collector.to_feeds(token_map)?,
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
            price_provider: Pyth::id(),
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

    /// Set execution fee.
    pub fn execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_lamports = lamports;
        self
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

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(
        &mut self,
        price_updates: crate::pyth::pull_oracle::Prices,
    ) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
        self
    }

    /// Insert an Address Lookup Table.
    pub fn add_alt(&mut self, account: AddressLookupTableAccount) -> &mut Self {
        self.alts.insert(account.key, account.addresses);
        self
    }

    async fn prepare_hint(&mut self) -> crate::Result<ExecuteGlvWithdrawalHint> {
        match &self.hint {
            Some(hint) => Ok(hint.clone()),
            None => {
                let glv_deposit = self
                    .client
                    .account::<ZeroCopy<GlvWithdrawal>>(&self.glv_withdrawal)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;

                let glv_address = self
                    .client
                    .find_glv_address(&glv_deposit.tokens().glv_token());
                let glv = self
                    .client
                    .account::<ZeroCopy<Glv>>(&glv_address)
                    .await?
                    .ok_or(crate::Error::NotFound)?
                    .0;

                let mut index_tokens = Vec::with_capacity(glv.num_markets());
                for token in glv.market_tokens() {
                    let market = self.client.find_market_address(glv.store(), &token);
                    let market = self.client.market(&market).await?;
                    index_tokens.push(market.meta().index_token_mint);
                }

                let store = glv_deposit.header().store();
                let token_map_address = self
                    .client
                    .authorized_token_map_address(store)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                let token_map = self.client.token_map(&token_map_address).await?;
                let hint = ExecuteGlvWithdrawalHint::new(
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

    /// Build.
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;

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
            .collect::<Result<Vec<_>, _>>()?;
        let markets = hint
            .swap
            .unique_market_tokens_excluding_current(&hint.market_token)
            .map(|mint| AccountMeta {
                pubkey: self.client.find_market_address(&hint.store, mint),
                is_signer: false,
                is_writable: true,
            });

        let glv_accounts = split_to_accounts(
            hint.glv_market_tokens.iter().copied(),
            &glv,
            &hint.store,
            &self.client.store_program_id(),
            &token_program_id,
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
            .store_rpc()
            .accounts(accounts::ExecuteGlvWithdrawal {
                authority,
                store: hint.store,
                token_map: hint.token_map,
                price_provider: self.price_provider,
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
            })
            .args(instruction::ExecuteGlvWithdrawal {
                execution_lamports: self.execution_lamports,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(glv_accounts)
            .accounts(feeds.into_iter().chain(markets).collect::<Vec<_>>())
            .compute_budget(
                ComputeBudget::default().with_limit(EXECUTE_GLV_WITHDRAWAL_COMPUTE_BUDGET),
            )
            .lookup_tables(self.alts.clone());

        if self.close {
            let close = self
                .client
                .close_glv_withdrawal(&self.glv_withdrawal)
                .reason("executed")
                .hint(hint.close)
                .build()
                .await?;
            Ok(execute.merge(close))
        } else {
            Ok(execute)
        }
    }
}

#[cfg(feature = "pyth-pull-oracle")]
mod pyth {
    use crate::pyth::{
        pull_oracle::{ExecuteWithPythPrices, Prices},
        PythPullOracleContext,
    };

    use super::*;

    impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
        for ExecuteGlvWithdrawalBuilder<'a, C>
    {
        fn set_execution_fee(&mut self, lamports: u64) {
            self.execution_fee(lamports);
        }

        async fn context(&mut self) -> crate::Result<PythPullOracleContext> {
            let hint = self.prepare_hint().await?;
            let ctx = PythPullOracleContext::try_from_feeds(&hint.feeds)?;
            Ok(ctx)
        }

        async fn build_rpc_with_price_updates(
            &mut self,
            price_updates: Prices,
        ) -> crate::Result<Vec<crate::utils::RpcBuilder<'a, C, ()>>> {
            let rpc = self
                .parse_with_pyth_price_updates(price_updates)
                .build()
                .await?;
            Ok(vec![rpc])
        }
    }
}
