/// Deposit.
pub mod deposit;

/// Withdrawal.
pub mod withdrawal;

/// Order.
pub mod order;

/// Shift.
pub mod shift;

/// Auto-deleveraging.
pub mod auto_deleveraging;

/// Position cut.
pub mod position_cut;

/// Treasury.
pub mod treasury;

use std::{future::Future, ops::Deref};

use anchor_client::{
    anchor_lang::system_program,
    solana_sdk::{pubkey::Pubkey, signer::Signer},
};
use auto_deleveraging::UpdateAdlBuilder;
use gmsol_store::{
    ops::order::PositionCutKind,
    states::{
        feature::{ActionDisabledFlag, DomainDisabledFlag},
        order::OrderKind,
        NonceBytes, UpdateOrderParams,
    },
};
use order::{CloseOrderBuilder, OrderParams};
use position_cut::PositionCutBuilder;
use rand::{distributions::Standard, Rng};
use shift::{CloseShiftBuilder, CreateShiftBuilder, ExecuteShiftBuilder};
use treasury::ClaimFeesBuilder;

use crate::{store::market::VaultOps, utils::RpcBuilder};

use self::{
    deposit::{CloseDepositBuilder, CreateDepositBuilder, ExecuteDepositBuilder},
    order::{CreateOrderBuilder, ExecuteOrderBuilder},
    withdrawal::{CloseWithdrawalBuilder, CreateWithdrawalBuilder, ExecuteWithdrawalBuilder},
};

/// Exchange instructions.
pub trait ExchangeOps<C> {
    /// Toggle feature.
    fn toggle_feature(
        &self,
        store: &Pubkey,
        domian: DomainDisabledFlag,
        action: ActionDisabledFlag,
        enable: bool,
    ) -> RpcBuilder<C>;

    /// Claim fees.
    fn claim_fees(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> ClaimFeesBuilder<C>;

    /// Create a new market and return its token mint address.
    #[allow(clippy::too_many_arguments)]
    fn create_market(
        &self,
        store: &Pubkey,
        name: &str,
        index_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        enable: bool,
        token_map: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<(RpcBuilder<C>, Pubkey)>>;

    /// Fund the given market.
    fn fund_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        source_account: &Pubkey,
        amount: u64,
        token: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<RpcBuilder<C>>>;

    /// Create a deposit.
    fn create_deposit(&self, store: &Pubkey, market_token: &Pubkey) -> CreateDepositBuilder<C>;

    /// Cancel a deposit.
    fn close_deposit(&self, store: &Pubkey, deposit: &Pubkey) -> CloseDepositBuilder<C>;

    /// Execute a deposit.
    fn execute_deposit(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteDepositBuilder<C>;

    /// Create a withdrawal.
    fn create_withdrawal(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateWithdrawalBuilder<C>;

    /// Close a withdrawal.
    fn close_withdrawal(&self, store: &Pubkey, withdrawal: &Pubkey) -> CloseWithdrawalBuilder<C>;

    /// Execute a withdrawal.
    fn execute_withdrawal(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteWithdrawalBuilder<C>;

    /// Create an order.
    fn create_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_output_token_long: bool,
        params: OrderParams,
    ) -> CreateOrderBuilder<C>;

    /// Update an order.
    fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
    ) -> crate::Result<RpcBuilder<C>>;

    /// Execute an order.
    fn execute_order(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        order: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> crate::Result<ExecuteOrderBuilder<C>>;

    /// Close an order.
    fn close_order(&self, order: &Pubkey) -> crate::Result<CloseOrderBuilder<C>>;

    /// Liquidate a position.
    fn liquidate(&self, oracle: &Pubkey, position: &Pubkey)
        -> crate::Result<PositionCutBuilder<C>>;

    /// Auto-deleverage a position.
    fn auto_deleverage(
        &self,
        oracle: &Pubkey,
        position: &Pubkey,
        size_delta_usd: u128,
    ) -> crate::Result<PositionCutBuilder<C>>;

    /// Update ADL state.
    fn update_adl(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
    ) -> crate::Result<UpdateAdlBuilder<C>>;

    /// Create a market increase position order.
    fn market_increase(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_collateral_token_long: bool,
        initial_collateral_amount: u64,
        is_long: bool,
        increment_size_in_usd: u128,
    ) -> CreateOrderBuilder<C> {
        let params = OrderParams {
            kind: OrderKind::MarketIncrease,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: increment_size_in_usd,
            initial_collateral_delta_amount: initial_collateral_amount,
            acceptable_price: None,
            trigger_price: None,
            is_long,
        };
        self.create_order(store, market_token, is_collateral_token_long, params)
    }

    /// Create a market decrease position order.
    fn market_decrease(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_collateral_token_long: bool,
        collateral_withdrawal_amount: u64,
        is_long: bool,
        decrement_size_in_usd: u128,
    ) -> CreateOrderBuilder<C> {
        let params = OrderParams {
            kind: OrderKind::MarketDecrease,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: decrement_size_in_usd,
            initial_collateral_delta_amount: collateral_withdrawal_amount,
            acceptable_price: None,
            trigger_price: None,
            is_long,
        };
        self.create_order(store, market_token, is_collateral_token_long, params)
    }

    /// Create a market swap order.
    fn market_swap<'a, S>(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_output_token_long: bool,
        initial_swap_in_token: &Pubkey,
        initial_swap_in_token_amount: u64,
        swap_path: impl IntoIterator<Item = &'a Pubkey>,
    ) -> CreateOrderBuilder<C>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        let params = OrderParams {
            kind: OrderKind::MarketSwap,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: 0,
            initial_collateral_delta_amount: initial_swap_in_token_amount,
            acceptable_price: None,
            trigger_price: None,
            is_long: true,
        };
        let mut builder = self.create_order(store, market_token, is_output_token_long, params);
        builder
            .initial_collateral_token(initial_swap_in_token, None)
            .swap_path(swap_path.into_iter().copied().collect());
        builder
    }

    /// Create a limit increase order.
    #[allow(clippy::too_many_arguments)]
    fn limit_increase(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
        increment_size_in_usd: u128,
        price: u128,
        is_collateral_token_long: bool,
        initial_collateral_amount: u64,
    ) -> CreateOrderBuilder<C> {
        let params = OrderParams {
            kind: OrderKind::LimitIncrease,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: increment_size_in_usd,
            initial_collateral_delta_amount: initial_collateral_amount,
            acceptable_price: None,
            trigger_price: Some(price),
            is_long,
        };
        self.create_order(store, market_token, is_collateral_token_long, params)
    }

    /// Create a limit decrease order.
    #[allow(clippy::too_many_arguments)]
    fn limit_decrease(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
        decrement_size_in_usd: u128,
        price: u128,
        is_collateral_token_long: bool,
        collateral_withdrawal_amount: u64,
    ) -> CreateOrderBuilder<C> {
        let params = OrderParams {
            kind: OrderKind::LimitDecrease,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: decrement_size_in_usd,
            initial_collateral_delta_amount: collateral_withdrawal_amount,
            acceptable_price: None,
            trigger_price: Some(price),
            is_long,
        };
        self.create_order(store, market_token, is_collateral_token_long, params)
    }

    /// Create a stop-loss decrease order.
    #[allow(clippy::too_many_arguments)]
    fn stop_loss(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
        decrement_size_in_usd: u128,
        price: u128,
        is_collateral_token_long: bool,
        collateral_withdrawal_amount: u64,
    ) -> CreateOrderBuilder<C> {
        let params = OrderParams {
            kind: OrderKind::StopLossDecrease,
            decrease_position_swap_type: None,
            min_output_amount: 0,
            size_delta_usd: decrement_size_in_usd,
            initial_collateral_delta_amount: collateral_withdrawal_amount,
            acceptable_price: None,
            trigger_price: Some(price),
            is_long,
        };
        self.create_order(store, market_token, is_collateral_token_long, params)
    }

    /// Create a limit swap order.
    #[allow(clippy::too_many_arguments)]
    fn limit_swap<'a, S>(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_output_token_long: bool,
        min_output_amount: u64,
        initial_swap_in_token: &Pubkey,
        initial_swap_in_token_amount: u64,
        swap_path: impl IntoIterator<Item = &'a Pubkey>,
    ) -> CreateOrderBuilder<C>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        let params = OrderParams {
            kind: OrderKind::LimitSwap,
            decrease_position_swap_type: None,
            min_output_amount: u128::from(min_output_amount),
            size_delta_usd: 0,
            initial_collateral_delta_amount: initial_swap_in_token_amount,
            acceptable_price: None,
            trigger_price: None,
            is_long: true,
        };
        let mut builder = self.create_order(store, market_token, is_output_token_long, params);
        builder
            .initial_collateral_token(initial_swap_in_token, None)
            .swap_path(swap_path.into_iter().copied().collect());
        builder
    }

    /// Create shift.
    fn create_shift(
        &self,
        store: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateShiftBuilder<C>;

    /// Close shift.
    fn close_shift(&self, shift: &Pubkey) -> CloseShiftBuilder<C>;

    /// Execute shift.
    fn execute_shift(&self, oracle: &Pubkey, shift: &Pubkey) -> ExecuteShiftBuilder<C>;
}

impl<S, C> ExchangeOps<C> for crate::Client<C>
where
    C: Deref<Target = S> + Clone,
    S: Signer,
{
    fn toggle_feature(
        &self,
        store: &Pubkey,
        domian: DomainDisabledFlag,
        action: ActionDisabledFlag,
        enable: bool,
    ) -> RpcBuilder<C> {
        self.store_rpc()
            .args(gmsol_store::instruction::ToggleFeature {
                domain: domian.to_string(),
                action: action.to_string(),
                enable,
            })
            .accounts(gmsol_store::accounts::ToggleFeature {
                authority: self.payer(),
                store: *store,
            })
    }

    fn claim_fees(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_long_token: bool,
    ) -> ClaimFeesBuilder<C> {
        ClaimFeesBuilder::new(self, store, market_token, is_long_token)
    }

    fn create_deposit(&self, store: &Pubkey, market_token: &Pubkey) -> CreateDepositBuilder<C> {
        CreateDepositBuilder::new(self, *store, *market_token)
    }

    fn close_deposit(&self, store: &Pubkey, deposit: &Pubkey) -> CloseDepositBuilder<C> {
        CloseDepositBuilder::new(self, store, deposit)
    }

    fn execute_deposit(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteDepositBuilder<C> {
        ExecuteDepositBuilder::new(self, store, oracle, deposit, cancel_on_execution_error)
    }

    fn create_withdrawal(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateWithdrawalBuilder<C> {
        CreateWithdrawalBuilder::new(self, *store, *market_token, amount)
    }

    fn close_withdrawal(&self, store: &Pubkey, withdrawal: &Pubkey) -> CloseWithdrawalBuilder<C> {
        CloseWithdrawalBuilder::new(self, store, withdrawal)
    }

    fn execute_withdrawal(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteWithdrawalBuilder<C> {
        ExecuteWithdrawalBuilder::new(self, store, oracle, withdrawal, cancel_on_execution_error)
    }

    async fn create_market(
        &self,
        store: &Pubkey,
        name: &str,
        index_token: &Pubkey,
        long_token: &Pubkey,
        short_token: &Pubkey,
        enable: bool,
        token_map: Option<&Pubkey>,
    ) -> crate::Result<(RpcBuilder<C>, Pubkey)> {
        let token_map = match token_map {
            Some(token_map) => *token_map,
            None => self
                .authorized_token_map_address(store)
                .await?
                .ok_or(crate::Error::NotFound)?,
        };
        let authority = self.payer();
        let market_token =
            self.find_market_token_address(store, index_token, long_token, short_token);
        let prepare_long_token_vault = self.initialize_market_vault(store, long_token).0;
        let prepare_short_token_vault = self.initialize_market_vault(store, short_token).0;
        let prepare_market_token_vault = self.initialize_market_vault(store, &market_token).0;
        let builder = self
            .store_rpc()
            .accounts(gmsol_store::accounts::InitializeMarket {
                authority,
                store: *store,
                token_map,
                market: self.find_market_address(store, &market_token),
                market_token_mint: market_token,
                long_token_mint: *long_token,
                short_token_mint: *short_token,
                long_token_vault: self.find_market_vault_address(store, long_token),
                short_token_vault: self.find_market_vault_address(store, short_token),
                system_program: system_program::ID,
                token_program: anchor_spl::token::ID,
            })
            .args(gmsol_store::instruction::InitializeMarket {
                name: name.to_string(),
                index_token_mint: *index_token,
                enable,
            });
        Ok((
            prepare_long_token_vault
                .merge(prepare_short_token_vault)
                .merge(builder)
                .merge(prepare_market_token_vault),
            market_token,
        ))
    }

    async fn fund_market(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        source_account: &Pubkey,
        amount: u64,
        token: Option<&Pubkey>,
    ) -> crate::Result<RpcBuilder<C>> {
        use anchor_spl::token::TokenAccount;

        let token = match token {
            Some(token) => *token,
            None => {
                let account = self
                    .account::<TokenAccount>(source_account)
                    .await?
                    .ok_or(crate::Error::NotFound)?;
                account.mint
            }
        };
        let vault = self.find_market_vault_address(store, &token);
        let market = self.find_market_address(store, market_token);
        Ok(self
            .store_rpc()
            .args(gmsol_store::instruction::MarketTransferIn { amount })
            .accounts(gmsol_store::accounts::MarketTransferIn {
                authority: self.payer(),
                from_authority: self.payer(),
                store: *store,
                market,
                vault,
                from: *source_account,
                token_program: anchor_spl::token::ID,
            }))
    }

    fn create_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_output_token_long: bool,
        params: OrderParams,
    ) -> CreateOrderBuilder<C> {
        CreateOrderBuilder::new(self, store, market_token, params, is_output_token_long)
    }

    fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
    ) -> crate::Result<RpcBuilder<C>> {
        Ok(self
            .store_rpc()
            .accounts(gmsol_store::accounts::UpdateOrder {
                owner: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
                order: *order,
            })
            .args(gmsol_store::instruction::UpdateOrder { params }))
    }

    fn execute_order(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        order: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> crate::Result<ExecuteOrderBuilder<C>> {
        ExecuteOrderBuilder::try_new(self, store, oracle, order, cancel_on_execution_error)
    }

    fn close_order(&self, order: &Pubkey) -> crate::Result<CloseOrderBuilder<C>> {
        Ok(CloseOrderBuilder::new(self, order))
    }

    fn liquidate(
        &self,
        oracle: &Pubkey,
        position: &Pubkey,
    ) -> crate::Result<PositionCutBuilder<C>> {
        PositionCutBuilder::try_new(self, PositionCutKind::Liquidate, oracle, position)
    }

    fn auto_deleverage(
        &self,
        oracle: &Pubkey,
        position: &Pubkey,
        size_delta_usd: u128,
    ) -> crate::Result<PositionCutBuilder<C>> {
        PositionCutBuilder::try_new(
            self,
            PositionCutKind::AutoDeleverage(size_delta_usd),
            oracle,
            position,
        )
    }

    fn update_adl(
        &self,
        store: &Pubkey,
        oracle: &Pubkey,
        market_token: &Pubkey,
        is_long: bool,
    ) -> crate::Result<UpdateAdlBuilder<C>> {
        UpdateAdlBuilder::try_new(self, store, oracle, market_token, is_long)
    }

    fn create_shift(
        &self,
        store: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateShiftBuilder<C> {
        CreateShiftBuilder::new(self, store, from_market_token, to_market_token, amount)
    }

    fn close_shift(&self, shift: &Pubkey) -> CloseShiftBuilder<C> {
        CloseShiftBuilder::new(self, shift)
    }

    fn execute_shift(&self, oracle: &Pubkey, shift: &Pubkey) -> ExecuteShiftBuilder<C> {
        ExecuteShiftBuilder::new(self, oracle, shift)
    }
}

pub(crate) fn generate_nonce() -> NonceBytes {
    rand::thread_rng()
        .sample_iter(Standard)
        .take(32)
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap()
}
