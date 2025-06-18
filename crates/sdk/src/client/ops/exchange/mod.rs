/// Builders for transactions related to deposits.
pub mod deposit;

/// Builders for transactions related to withdrawals.
pub mod withdrawal;

/// Builders for transactions related to shifts.
pub mod shift;

/// Builders for transactions related to orders.
pub mod order;

/// Builders for transactions related to GLV deposits.
pub mod glv_deposit;

/// Builders for transactions related to GLV withdrawals.
pub mod glv_withdrawal;

/// Builders for transactions related to GLV shifts.
pub mod glv_shift;

use std::{future::Future, ops::Deref};

use deposit::{CloseDepositBuilder, CreateDepositBuilder, ExecuteDepositBuilder};
use glv_deposit::{CloseGlvDepositBuilder, CreateGlvDepositBuilder, ExecuteGlvDepositBuilder};
use glv_shift::{CloseGlvShiftBuilder, CreateGlvShiftBuilder, ExecuteGlvShiftBuilder};
use glv_withdrawal::{
    CloseGlvWithdrawalBuilder, CreateGlvWithdrawalBuilder, ExecuteGlvWithdrawalBuilder,
};
use gmsol_programs::gmsol_store::{
    client::{accounts, args},
    types::UpdateOrderParams,
};
use gmsol_solana_utils::transaction_builder::TransactionBuilder;
use gmsol_utils::order::{OrderKind, PositionCutKind};
use order::{
    CloseOrderBuilder, CreateOrderBuilder, ExecuteOrderBuilder, OrderParams, PositionCutBuilder,
    UpdateAdlBuilder,
};
use shift::{CloseShiftBuilder, CreateShiftBuilder, ExecuteShiftBuilder};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use withdrawal::{CloseWithdrawalBuilder, CreateWithdrawalBuilder, ExecuteWithdrawalBuilder};

use crate::{
    builders::callback::{Callback, CallbackParams},
    client::Client,
};

/// Exchange operations.
pub trait ExchangeOps<C> {
    /// Create a deposit.
    fn create_deposit(&self, store: &Pubkey, market_token: &Pubkey) -> CreateDepositBuilder<C>;

    /// Create first deposit.
    fn create_first_deposit(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateDepositBuilder<C>;

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
    fn execute_shift(
        &self,
        oracle: &Pubkey,
        shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteShiftBuilder<C>;

    /// Create an order.
    fn create_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        is_output_token_long: bool,
        params: OrderParams,
    ) -> CreateOrderBuilder<C>;

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
            valid_from_ts: None,
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
            valid_from_ts: None,
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
            valid_from_ts: None,
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
            valid_from_ts: None,
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
            valid_from_ts: None,
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
            valid_from_ts: None,
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
            valid_from_ts: None,
        };
        let mut builder = self.create_order(store, market_token, is_output_token_long, params);
        builder
            .initial_collateral_token(initial_swap_in_token, None)
            .swap_path(swap_path.into_iter().copied().collect());
        builder
    }

    /// Update an order.
    fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
        hint: Option<Option<Callback>>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

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

    /// Cancel order if the position does not exist.
    fn cancel_order_if_no_position(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        position_hint: Option<&Pubkey>,
    ) -> impl Future<Output = crate::Result<TransactionBuilder<C>>>;

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
        for_long: bool,
        for_short: bool,
    ) -> crate::Result<UpdateAdlBuilder<C>>;

    /// Create a GLV deposit.
    fn create_glv_deposit(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateGlvDepositBuilder<C>;

    /// Close a GLV deposit.
    fn close_glv_deposit(&self, glv_deposit: &Pubkey) -> CloseGlvDepositBuilder<C>;

    /// Execute the given GLV deposit.
    fn execute_glv_deposit(
        &self,
        oracle: &Pubkey,
        glv_deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvDepositBuilder<C>;

    fn create_glv_withdrawal(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvWithdrawalBuilder<C>;

    /// Close a GLV withdrawal.
    fn close_glv_withdrawal(&self, glv_withdrawal: &Pubkey) -> CloseGlvWithdrawalBuilder<C>;

    /// Execute the given GLV deposit.
    fn execute_glv_withdrawal(
        &self,
        oracle: &Pubkey,
        glv_withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvWithdrawalBuilder<C>;

    fn create_glv_shift(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvShiftBuilder<C>;

    fn close_glv_shift(&self, glv_shift: &Pubkey) -> CloseGlvShiftBuilder<C>;

    fn execute_glv_shift(
        &self,
        oracle: &Pubkey,
        glv_shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvShiftBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> ExchangeOps<C> for Client<C> {
    fn create_deposit(&self, store: &Pubkey, market_token: &Pubkey) -> CreateDepositBuilder<C> {
        CreateDepositBuilder::new(self, *store, *market_token)
    }

    fn create_first_deposit(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateDepositBuilder<C> {
        let mut builder = self.create_deposit(store, market_token);
        builder.receiver(Some(self.find_first_deposit_owner_address()));
        builder
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

    fn execute_shift(
        &self,
        oracle: &Pubkey,
        shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteShiftBuilder<C> {
        ExecuteShiftBuilder::new(self, oracle, shift, cancel_on_execution_error)
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

    async fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
        hint: Option<Option<Callback>>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let callback = match hint {
            Some(callback) => callback,
            None => {
                let order = self.order(order).await?;
                Callback::from_header(&order.header)?
            }
        };
        let CallbackParams {
            callback_authority,
            callback_program,
            callback_shared_data_account,
            callback_partitioned_data_account,
            ..
        } = self.get_callback_params(callback.as_ref());
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::UpdateOrderV2 {
                owner: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
                order: *order,
                event_authority: self.store_event_authority(),
                program: *self.store_program_id(),
                callback_authority,
                callback_program,
                callback_shared_data_account,
                callback_partitioned_data_account,
            })
            .anchor_args(args::UpdateOrderV2 { params }))
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

    async fn cancel_order_if_no_position(
        &self,
        store: &Pubkey,
        order: &Pubkey,
        position_hint: Option<&Pubkey>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let position = match position_hint {
            Some(position) => *position,
            None => {
                let order = self.order(order).await?;

                let position = order
                    .params
                    .position()
                    .ok_or_else(|| crate::Error::custom("this order does not have position"))?;

                *position
            }
        };

        Ok(self
            .store_transaction()
            .anchor_args(args::CancelOrderIfNoPosition {})
            .anchor_accounts(accounts::CancelOrderIfNoPosition {
                authority: self.payer(),
                store: *store,
                order: *order,
                position,
            }))
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
        for_long: bool,
        for_short: bool,
    ) -> crate::Result<UpdateAdlBuilder<C>> {
        UpdateAdlBuilder::try_new(self, store, oracle, market_token, for_long, for_short)
    }

    fn create_glv_deposit(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> CreateGlvDepositBuilder<C> {
        CreateGlvDepositBuilder::new(self, *store, *glv_token, *market_token)
    }

    fn close_glv_deposit(&self, glv_deposit: &Pubkey) -> CloseGlvDepositBuilder<C> {
        CloseGlvDepositBuilder::new(self, *glv_deposit)
    }

    fn execute_glv_deposit(
        &self,
        oracle: &Pubkey,
        glv_deposit: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvDepositBuilder<C> {
        ExecuteGlvDepositBuilder::new(self, *oracle, *glv_deposit, cancel_on_execution_error)
    }

    fn create_glv_withdrawal(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvWithdrawalBuilder<C> {
        CreateGlvWithdrawalBuilder::new(self, *store, *glv_token, *market_token, amount)
    }

    fn close_glv_withdrawal(&self, glv_withdrawal: &Pubkey) -> CloseGlvWithdrawalBuilder<C> {
        CloseGlvWithdrawalBuilder::new(self, *glv_withdrawal)
    }

    fn execute_glv_withdrawal(
        &self,
        oracle: &Pubkey,
        glv_withdrawal: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvWithdrawalBuilder<C> {
        ExecuteGlvWithdrawalBuilder::new(self, *oracle, *glv_withdrawal, cancel_on_execution_error)
    }

    fn create_glv_shift(
        &self,
        store: &Pubkey,
        glv_token: &Pubkey,
        from_market_token: &Pubkey,
        to_market_token: &Pubkey,
        amount: u64,
    ) -> CreateGlvShiftBuilder<C> {
        CreateGlvShiftBuilder::new(
            self,
            store,
            glv_token,
            from_market_token,
            to_market_token,
            amount,
        )
    }

    fn close_glv_shift(&self, glv_shift: &Pubkey) -> CloseGlvShiftBuilder<C> {
        CloseGlvShiftBuilder::new(self, glv_shift)
    }

    fn execute_glv_shift(
        &self,
        oracle: &Pubkey,
        glv_shift: &Pubkey,
        cancel_on_execution_error: bool,
    ) -> ExecuteGlvShiftBuilder<C> {
        let mut builder = ExecuteGlvShiftBuilder::new(self, oracle, glv_shift);
        builder.cancel_on_execution_error(cancel_on_execution_error);
        builder
    }
}
