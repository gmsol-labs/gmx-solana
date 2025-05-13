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
use gmsol_utils::order::PositionCutKind;
use order::{
    CloseOrderBuilder, CreateOrderBuilder, ExecuteOrderBuilder, OrderParams, PositionCutBuilder,
    UpdateAdlBuilder,
};
use shift::{CloseShiftBuilder, CreateShiftBuilder, ExecuteShiftBuilder};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use withdrawal::{CloseWithdrawalBuilder, CreateWithdrawalBuilder, ExecuteWithdrawalBuilder};

use crate::client::Client;

/// Exchange operations.
pub trait ExchangeOps<C> {
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

    /// Update an order.
    fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
    ) -> crate::Result<TransactionBuilder<C>>;

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

    fn update_order(
        &self,
        store: &Pubkey,
        market_token: &Pubkey,
        order: &Pubkey,
        params: UpdateOrderParams,
    ) -> crate::Result<TransactionBuilder<C>> {
        Ok(self
            .store_transaction()
            .anchor_accounts(accounts::UpdateOrder {
                owner: self.payer(),
                store: *store,
                market: self.find_market_address(store, market_token),
                order: *order,
            })
            .anchor_args(args::UpdateOrder { params }))
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
                    .ok_or_else(|| crate::Error::unknown("this order does not have position"))?;

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
