use std::ops::Deref;

use anchor_client::{
    anchor_lang::{system_program, Id},
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use anchor_spl::associated_token::get_associated_token_address;
use gmsol_store::{
    accounts, instruction,
    instructions::ordered_tokens,
    ops::shift::CreateShiftParams,
    states::{
        common::{action::Action, TokensWithFeed},
        HasMarketMeta, NonceBytes, Pyth, Shift, Store, TokenMapAccess,
    },
};

use crate::{
    exchange::generate_nonce,
    store::{token::TokenAccountOps, utils::FeedsParser},
    utils::{RpcBuilder, ZeroCopy},
};

use super::ExchangeOps;

#[cfg(feature = "pyth-pull-oracle")]
use crate::pyth::pull_oracle::Prices;

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
            execution_fee: Shift::MIN_EXECUTION_LAMPORTS,
            amount,
            min_to_market_token_amount: 0,
            nonce: None,
            hint: Default::default(),
        }
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

    /// Build a [`RpcBuilder`] to create shift account and return the address of the shift account to create.
    pub fn build_with_address(&self) -> crate::Result<(RpcBuilder<'a, C>, Pubkey)> {
        let owner = self.client.payer();
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
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
        let to_market_token_ata = get_associated_token_address(&owner, &self.to_market_token);

        let prepare_escrow = self
            .client
            .store_rpc()
            .accounts(accounts::PrepareShiftEscorw {
                owner,
                store: self.store,
                shift,
                from_market_token: self.from_market_token,
                to_market_token: self.to_market_token,
                from_market_token_escrow,
                to_market_token_escrow,
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .args(instruction::PrepareShiftEscrow { nonce });

        let prepare_ata = self.client.prepare_associated_token_account(
            &self.to_market_token,
            &anchor_spl::token::ID,
            None,
        );

        let rpc = self
            .client
            .store_rpc()
            .accounts(accounts::CreateShift {
                owner,
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
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
            })
            .args(instruction::CreateShift {
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
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    from_market_token_escrow: Pubkey,
    to_market_token_escrow: Pubkey,
}

impl CloseShiftHint {
    /// Create hint for `close_shift` instruction.
    pub fn new(shift: &Shift) -> crate::Result<Self> {
        let tokens = shift.tokens();
        Ok(Self {
            store: *shift.header().store(),
            owner: *shift.header().owner(),
            from_market_token: tokens.from_market_token(),
            from_market_token_escrow: tokens.from_market_token_account(),
            to_market_token: tokens.to_market_token(),
            to_market_token_escrow: tokens.to_market_token_account(),
        })
    }

    #[allow(clippy::wrong_self_convention)]
    fn from_market_token_ata(&self) -> Pubkey {
        get_associated_token_address(&self.owner, &self.from_market_token)
    }

    fn to_market_token_ata(&self) -> Pubkey {
        get_associated_token_address(&self.owner, &self.to_market_token)
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

    /// Build a [`RpcBuilder`] to close shift account.
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
        let hint = self.prepare_hint().await?;
        let executor = self.client.payer();
        let rpc = self
            .client
            .store_rpc()
            .accounts(accounts::CloseShift {
                executor,
                store: hint.store,
                owner: hint.owner,
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
                program: self.client.store_program_id(),
            })
            .args(instruction::CloseShift {
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
    price_provider: Pubkey,
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
    from_market_token: Pubkey,
    to_market_token: Pubkey,
    from_market_token_escrow: Pubkey,
    to_market_token_escrow: Pubkey,
    /// Feeds.
    pub feeds: TokensWithFeed,
}

impl ExecuteShiftHint {
    /// Create hint for `execute_shift` instruction.
    pub fn new(
        shift: &Shift,
        store: &Store,
        map: &impl TokenMapAccess,
        from_market: &impl HasMarketMeta,
        to_market: &impl HasMarketMeta,
    ) -> crate::Result<Self> {
        use gmsol_store::states::common::token_with_feeds::token_records;

        let token_infos = shift.tokens();

        let ordered_tokens = ordered_tokens(from_market, to_market);
        let token_records = token_records(map, &ordered_tokens)?;
        let feeds = TokensWithFeed::try_from_records(token_records)?;

        Ok(Self {
            store: *shift.header().store(),
            owner: *shift.header().owner(),
            from_market_token: token_infos.from_market_token(),
            from_market_token_escrow: token_infos.from_market_token_account(),
            to_market_token: token_infos.to_market_token(),
            to_market_token_escrow: token_infos.to_market_token_account(),
            token_map: *store.token_map().ok_or(crate::Error::NotFound)?,
            feeds,
        })
    }
}

impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteShiftBuilder<'a, C> {
    pub(super) fn new(client: &'a crate::Client<C>, oracle: &Pubkey, shift: &Pubkey) -> Self {
        Self {
            client,
            shift: *shift,
            hint: None,
            execution_fee: 0,
            cancel_on_execution_error: true,
            price_provider: Pyth::id(),
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

    /// Set execution fee.
    pub fn execution_fee(&mut self, lamports: u64) -> &mut Self {
        self.execution_fee = lamports;
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

    /// Parse feeds with the given price udpates map.
    #[cfg(feature = "pyth-pull-oracle")]
    pub fn parse_with_pyth_price_updates(&mut self, price_updates: Prices) -> &mut Self {
        self.feeds_parser.with_pyth_price_updates(price_updates);
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
                let store = self.client.store(shift.0.header().store()).await?;
                let token_map_address = store
                    .token_map()
                    .ok_or(crate::Error::invalid_argument("token map is not set"))?;
                let token_map = self.client.token_map(token_map_address).await?;
                let from_market_token = shift.0.tokens().from_market_token();
                let to_market_token = shift.0.tokens().to_market_token();
                let from_market = self
                    .client
                    .find_market_address(shift.0.header().store(), &from_market_token);
                let from_market = self.client.market(&from_market).await?;
                let to_market = self
                    .client
                    .find_market_address(shift.0.header().store(), &to_market_token);
                let to_market = self.client.market(&to_market).await?;
                let hint =
                    ExecuteShiftHint::new(&shift.0, &store, &token_map, &from_market, &to_market)?;
                self.hint = Some(hint.clone());
                hint
            }
        };

        Ok(hint)
    }

    /// Build a [`RpcBuilder`] for `execute_shift` instruction.
    pub async fn build(&mut self) -> crate::Result<RpcBuilder<'a, C>> {
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

        let feeds = self
            .feeds_parser
            .parse(&hint.feeds)
            .collect::<Result<Vec<_>, _>>()?;

        let mut rpc = self
            .client
            .store_rpc()
            .accounts(accounts::ExecuteShift {
                authority,
                store: hint.store,
                token_map: hint.token_map,
                price_provider: self.price_provider,
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
            })
            .args(instruction::ExecuteShift {
                execution_lamports: self.execution_fee,
                throw_on_execution_error: !self.cancel_on_execution_error,
            })
            .accounts(feeds);

        if self.close {
            let close = self
                .client
                .close_shift(&self.shift)
                .hint(CloseShiftHint {
                    store: hint.store,
                    owner: hint.owner,
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

#[cfg(feature = "pyth-pull-oracle")]
mod pyth {
    use crate::pyth::{pull_oracle::ExecuteWithPythPrices, PythPullOracleContext};

    use super::*;

    impl<'a, C: Deref<Target = impl Signer> + Clone> ExecuteWithPythPrices<'a, C>
        for ExecuteShiftBuilder<'a, C>
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
