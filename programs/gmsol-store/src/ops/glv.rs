use std::borrow::Borrow;

use anchor_lang::prelude::*;
use anchor_spl::{
    token::{transfer_checked, Mint, TokenAccount, TransferChecked},
    token_interface,
};
use gmsol_model::{price::Prices, BaseMarketExt, PnlFactorKind};
use typed_builder::TypedBuilder;

use crate::{
    constants,
    events::{EventEmitter, GlvPricing, GlvPricingKind},
    states::{
        common::{
            action::{Action, ActionExt, ActionParams, ActionSigner},
            swap::SwapActionParamsExt,
        },
        glv::{GlvShift, GlvWithdrawal},
        market::revertible::Revertible,
        withdrawal::WithdrawalActionParams,
        Glv, GlvDeposit, HasMarketMeta, Market, NonceBytes, Oracle, Shift, Store,
        ValidateOracleTime,
    },
    utils::internal::TransferUtils,
    CoreError, CoreResult, ModelError,
};

use super::market::{Execute, RevertibleLiquidityMarketOperation};

/// Create GLV Deposit Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateGlvDepositParams {
    /// Execution fee in lamports
    pub execution_lamports: u64,
    /// The length of the swap path for long token.
    pub long_token_swap_length: u8,
    /// The length of the swap path for short token.
    pub short_token_swap_length: u8,
    /// Initial long token amount to deposit.
    pub initial_long_token_amount: u64,
    /// Initial short otken amount to deposit.
    pub initial_short_token_amount: u64,
    /// Market token amount.
    pub market_token_amount: u64,
    /// Minimum acceptable maount of market tokens to be minted.
    pub min_market_token_amount: u64,
    /// Minimum acceptable amount of glv tokens to receive.
    pub min_glv_token_amount: u64,
    /// Whether to unwrap native token when sending funds back.
    pub should_unwrap_native_token: bool,
}

impl ActionParams for CreateGlvDepositParams {
    fn execution_lamports(&self) -> u64 {
        self.execution_lamports
    }
}

/// Operation for creating GLV deposit.
#[derive(TypedBuilder)]
pub(crate) struct CreateGlvDepositOperation<'a, 'info> {
    glv_deposit: AccountLoader<'info, GlvDeposit>,
    market: AccountLoader<'info, Market>,
    store: AccountLoader<'info, Store>,
    owner: &'a AccountInfo<'info>,
    receiver: &'a AccountInfo<'info>,
    nonce: &'a NonceBytes,
    bump: u8,
    initial_long_token: Option<&'a Account<'info, TokenAccount>>,
    initial_short_token: Option<&'a Account<'info, TokenAccount>>,
    market_token: &'a Account<'info, TokenAccount>,
    glv_token: &'a InterfaceAccount<'info, token_interface::TokenAccount>,
    params: &'a CreateGlvDepositParams,
    swap_paths: &'info [AccountInfo<'info>],
}

impl CreateGlvDepositOperation<'_, '_> {
    /// Execute.
    ///
    /// # CHECK
    /// - The address of `glv_deposit` must be derived from the given `store`,
    ///   `owner`, `nonce` and `bump`.
    /// - `glv` must be owned by the given `store`.
    /// - `glv` must contain the given `market`.
    /// - All the token accounts must be owned by `glv_deposit`.
    /// - The mint of `market_token` account must be the market token of the
    ///   given `market`.
    /// - The mint of `glv_token` account must be the glv token of the given `glv`.
    ///
    /// # Errors
    /// - `market` must be initialized, enabled and owned by the given `store`.
    /// - `initial_long_token_amount`, `initial_short_token_amount` and `market_token_amount`
    ///   must not all be zero.
    /// - When the token amount is not zero, the corresponding token account must be provided and
    ///   have enough amount of tokens.
    /// - `execution_lamports` must greater than `MIN_EXECUTION_LAMPORTS` and there must be enough lamports
    ///   in the `glv_deposit` account.
    /// - `glv_deposit` must be uninitialized.
    /// - `swap_paths` must be valid.
    pub(crate) fn unchecked_execute(self) -> Result<()> {
        let (long_token, short_token) = self.validate_market_and_get_tokens()?;

        self.validate_params_excluding_swap()?;

        let id = self
            .market
            .load_mut()?
            .indexer_mut()
            .next_glv_deposit_id()?;

        let mut glv_deposit = self.glv_deposit.load_init()?;

        glv_deposit.header.init(
            id,
            self.store.key(),
            self.market.key(),
            self.owner.key(),
            self.receiver.key(),
            *self.nonce,
            self.bump,
            self.params.execution_lamports,
            self.params.should_unwrap_native_token,
        )?;

        // Init tokens and token accounts.
        let primary_token_in = if let Some(account) = self.initial_long_token {
            glv_deposit.tokens.initial_long_token.init(account);
            account.mint
        } else {
            long_token
        };

        let secondary_token_in = if let Some(account) = self.initial_short_token {
            glv_deposit.tokens.initial_short_token.init(account);
            account.mint
        } else {
            short_token
        };
        glv_deposit.tokens.market_token.init(self.market_token);
        glv_deposit
            .tokens
            .glv_token
            .init_with_interface(self.glv_token);

        // Init params.
        glv_deposit.params.deposit.initial_long_token_amount =
            self.params.initial_long_token_amount;
        glv_deposit.params.deposit.initial_short_token_amount =
            self.params.initial_short_token_amount;
        glv_deposit.params.deposit.min_market_token_amount = self.params.min_market_token_amount;

        glv_deposit.params.market_token_amount = self.params.market_token_amount;
        glv_deposit.params.min_glv_token_amount = self.params.min_glv_token_amount;

        // Init swap paths.
        glv_deposit.swap.validate_and_init(
            &*self.market.load()?,
            self.params.long_token_swap_length,
            self.params.short_token_swap_length,
            self.swap_paths,
            &self.store.key(),
            (&primary_token_in, &secondary_token_in),
            (&long_token, &short_token),
        )?;

        Ok(())
    }

    fn validate_market_and_get_tokens(&self) -> Result<(Pubkey, Pubkey)> {
        let market = self.market.load()?;
        let meta = market.validated_meta(&self.store.key())?;
        Ok((meta.long_token_mint, meta.short_token_mint))
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        let params = self.params;
        require!(
            params.initial_long_token_amount != 0
                || params.initial_short_token_amount != 0
                || params.market_token_amount != 0,
            CoreError::EmptyDeposit
        );

        if params.initial_long_token_amount != 0 {
            let Some(account) = self.initial_long_token.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            require_gte!(
                account.amount,
                params.initial_long_token_amount,
                CoreError::NotEnoughTokenAmount
            );
        }

        if params.initial_short_token_amount != 0 {
            let Some(account) = self.initial_short_token.as_ref() else {
                return err!(CoreError::TokenAccountNotProvided);
            };
            require_gte!(
                account.amount,
                params.initial_short_token_amount,
                CoreError::NotEnoughTokenAmount
            );
        }

        // If the two token accounts are actually the same, then we should check for the sum.
        let same_initial_token_amount = self.initial_long_token.as_ref().and_then(|long| {
            self.initial_short_token
                .as_ref()
                .and_then(|short| (long.key() == short.key()).then(|| long.amount))
        });
        if let Some(amount) = same_initial_token_amount {
            let total_amount = params
                .initial_long_token_amount
                .checked_add(params.initial_short_token_amount)
                .ok_or_else(|| error!(CoreError::TokenAmountExceedsLimit))?;
            require_gte!(amount, total_amount, CoreError::NotEnoughTokenAmount);
        }

        ActionExt::validate_balance(&self.glv_deposit, params.execution_lamports)?;

        Ok(())
    }
}

/// Operation for executing a GLV deposit.
#[derive(TypedBuilder)]
pub(crate) struct ExecuteGlvDepositOperation<'a, 'info> {
    glv_deposit: AccountLoader<'info, GlvDeposit>,
    token_program: AccountInfo<'info>,
    glv_token_program: AccountInfo<'info>,
    throw_on_execution_error: bool,
    store: AccountLoader<'info, Store>,
    glv: AccountLoader<'info, Glv>,
    glv_token_mint: &'a mut InterfaceAccount<'info, token_interface::Mint>,
    glv_token_receiver: AccountInfo<'info>,
    market: AccountLoader<'info, Market>,
    market_token_mint: &'a mut Account<'info, Mint>,
    market_token_source: &'a Account<'info, TokenAccount>,
    market_token_vault: AccountInfo<'info>,
    markets: &'info [AccountInfo<'info>],
    market_tokens: &'info [AccountInfo<'info>],
    oracle: &'a Oracle,
    remaining_accounts: &'info [AccountInfo<'info>],
    #[builder(setter(into))]
    event_emitter: EventEmitter<'a, 'info>,
}

impl ExecuteGlvDepositOperation<'_, '_> {
    /// Execute.
    ///
    /// # CHECK
    /// - The `glv_deposit` must be owned by the `store`.
    /// - The `glv` must be owned by the `store`, and be the GLV account of the `glv_deposit`.
    /// - The `market_token_mint` must be the market token of the `market`.
    /// - The `market_token_vault` must be the vault of GLV for the `market_token`.
    /// - The lengths of `markets` and `market_tokens` must be the same as the market tokens list of the `glv`.
    /// - The order of `markets` and `market_tokens` must be the same of the market tokens list of the `glv`.
    /// - The required prices of tokens must have been validated and stored in the `oracle`.
    ///
    /// # Errors
    /// - The `market` must be owned by the `store` and be the current market of the `glv_deposit`.
    /// - The swap markets provided by `remaining_accounts` must be valid.
    pub(crate) fn unchecked_execute(mut self) -> Result<bool> {
        let throw_on_execution_error = self.throw_on_execution_error;
        match self.validate_oracle() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired) if !throw_on_execution_error => {
                msg!(
                    "GLV Deposit expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok(false);
            }
            Err(err) => {
                return Err(error!(err));
            }
        }
        let executed = match self.perform_glv_deposit() {
            Ok(()) => true,
            Err(err) if !throw_on_execution_error => {
                msg!("Execute GLV deposit error: {}", err);
                false
            }
            Err(err) => return Err(err),
        };

        self.validate_after_execution()?;

        Ok(executed)
    }

    fn validate_oracle(&self) -> CoreResult<()> {
        self.oracle.validate_time(self)
    }

    fn validate_before_execution(&self) -> Result<()> {
        let market = self.market.load()?;
        market.validate(&self.store.key())?;

        let glv = self.glv.load()?;
        let glv_deposit = self.glv_deposit.load()?;

        glv_deposit.unchecked_validate_for_execution(self.glv_token_mint, &glv)?;

        require_gte!(
            self.market_token_source.amount,
            glv_deposit.params.market_token_amount,
            CoreError::NotEnoughTokenAmount,
        );

        Ok(())
    }

    fn validate_after_execution(&self) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        let glv = self.glv.load()?;
        let market_token = self.market_token_mint.key();
        let vault_balance = amount(&self.market_token_vault)?;

        require_gte!(
            vault_balance,
            glv.market_config(&market_token)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance(),
            CoreError::Internal
        );

        Ok(())
    }

    #[inline(never)]
    fn perform_glv_deposit(&mut self) -> Result<()> {
        use gmsol_model::utils::usd_to_market_token_amount;

        self.validate_before_execution()?;

        let glv_token_amount = {
            let deposit = self.glv_deposit.load()?;
            let mut market_token_amount = deposit.params.market_token_amount;

            let mut market = RevertibleLiquidityMarketOperation::new(
                &self.store,
                self.oracle,
                &self.market,
                self.market_token_mint,
                self.token_program.clone(),
                Some(&deposit.swap),
                self.remaining_accounts,
                self.event_emitter,
            )?;

            let mut op = market.op()?;

            if market_token_amount != 0 {
                // The Max PnL validation here helps prevent a malicious user from using GLV
                // to "withdraw" from a high-risk market, something that would normally be
                // blocked in other paths (like withdrawals or shift-withdrawal).
                let prices = self.oracle.market_prices(op.market())?;
                op.market()
                    .validate_max_pnl(
                        &prices,
                        PnlFactorKind::MaxAfterWithdrawal,
                        PnlFactorKind::MaxAfterWithdrawal,
                    )
                    .map_err(ModelError::from)?;
            }

            if deposit.is_market_deposit_required() {
                let executed = op.unchecked_deposit(
                    &deposit.header().receiver(),
                    &self.market_token_vault,
                    &deposit.params.deposit,
                    (
                        deposit.tokens.initial_long_token.token(),
                        deposit.tokens.initial_short_token.token(),
                    ),
                    None,
                )?;

                market_token_amount = market_token_amount
                    .checked_add(executed.output)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

                op = executed.with_output(());
            }

            let market_token_mint = op.market().market_meta().market_token_mint;
            let next_market_token_balance = self
                .glv
                .load()?
                .market_config(&market_token_mint)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance()
                .checked_add(market_token_amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;

            // Calculate GLV token amount to mint.
            let glv_amount = {
                let maximize_value = true;
                let glv_supply = self.glv_token_mint.supply;
                let glv_value = unchecked_get_glv_value(
                    &*self.glv.load()?,
                    self.oracle,
                    &op,
                    self.markets,
                    self.market_tokens,
                    maximize_value,
                )?;

                let (received_value, market_pool_value, market_token_supply) = {
                    let mut prices = self.oracle.market_prices(op.market())?;
                    let balance = u128::from(market_token_amount);
                    let received_value = get_glv_value_for_market_with_new_index_price(
                        self.oracle,
                        &mut prices,
                        op.market(),
                        balance,
                        false,
                    )?
                    .0;

                    let (_, market_pool_value, market_token_supply) =
                        get_glv_value_for_market(&prices, op.market(), balance, true)?;

                    (received_value, market_pool_value, market_token_supply)
                };

                // Validate market token balance.
                self.glv.load()?.validate_market_token_balance(
                    &market_token_mint,
                    next_market_token_balance,
                    &market_pool_value,
                    &market_token_supply,
                )?;

                msg!(
                    "[GLV] Calculating GLV amount with glv_supply={}, glv_value={}, received_value={}",
                    glv_supply,
                    glv_value,
                    received_value,
                );

                let glv_amount = usd_to_market_token_amount(
                    received_value,
                    glv_value,
                    u128::from(glv_supply),
                    constants::MARKET_USD_TO_AMOUNT_DIVISOR,
                )
                .ok_or_else(|| error!(CoreError::FailedToCalculateGlvAmountToMint))?;
                let output_amount = u64::try_from(glv_amount)
                    .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

                self.event_emitter.emit_cpi(&GlvPricing {
                    glv_token: self.glv_token_mint.key(),
                    market_token: market_token_mint.key(),
                    supply: glv_supply,
                    value_maximized: maximize_value,
                    value: glv_value,
                    input_amount: market_token_amount,
                    input_value: received_value,
                    output_amount,
                    kind: GlvPricingKind::Deposit,
                })?;

                output_amount
            };

            deposit.validate_output_amount(glv_amount)?;

            // Update market token balance.
            self.glv
                .load_mut()?
                .update_market_token_balance(&market_token_mint, next_market_token_balance)?;

            op.commit();

            glv_amount
        };

        // Invertible operations after the commitment.
        {
            // Complete the market tokens transfer.
            self.transfer_market_tokens_in();

            // Mint GLV token to the receiver.
            self.mint_glv_tokens(glv_token_amount);
        }

        Ok(())
    }

    /// Mint GLV tokens to target account.
    ///
    /// # Panic
    /// This is an invertible operation that will panic on error.
    fn mint_glv_tokens(&self, glv_token_amount: u64) {
        if glv_token_amount != 0 {
            TransferUtils::new(
                self.glv_token_program.clone(),
                &self.store,
                self.glv_token_mint.to_account_info(),
            )
            .mint_to(&self.glv_token_receiver, glv_token_amount)
            .expect("failed to mint glv tokens");
        }
    }

    /// Transfer market tokens to vault.
    ///
    /// # Panic
    /// This is an invertible operation that will panic on error.
    fn transfer_market_tokens_in(&self) {
        use anchor_spl::token_interface::{transfer_checked, TransferChecked};

        let deposit = self.glv_deposit.load().expect("must have been checked");
        let signer = deposit.signer();

        let amount = deposit.params.market_token_amount;
        if amount != 0 {
            let token = &*self.market_token_mint;
            let from = &self.market_token_source;
            let to = &self.market_token_vault;
            let ctx = CpiContext::new(
                self.token_program.to_account_info(),
                TransferChecked {
                    from: from.to_account_info(),
                    mint: token.to_account_info(),
                    to: to.to_account_info(),
                    authority: self.glv_deposit.to_account_info(),
                },
            );
            transfer_checked(
                ctx.with_signer(&[&signer.as_seeds()]),
                amount,
                token.decimals,
            )
            .expect("failed to transfer market tokens");
        }
    }
}

impl ValidateOracleTime for ExecuteGlvDepositOperation<'_, '_> {
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>> {
        Ok(Some(
            self.glv_deposit
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header
                .updated_at,
        ))
    }

    fn oracle_updated_before(&self) -> CoreResult<Option<i64>> {
        let ts = self
            .store
            .load()
            .map_err(|_| CoreError::LoadAccountError)?
            .request_expiration_at(
                self.glv_deposit
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .header
                    .updated_at,
            )?;
        Ok(Some(ts))
    }

    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>> {
        Ok(Some(
            self.glv_deposit
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header
                .updated_at_slot,
        ))
    }
}

/// Create GLV Withdrawal Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateGlvWithdrawalParams {
    /// Execution fee in lamports
    pub execution_lamports: u64,
    /// The length of the swap path for long token.
    pub long_token_swap_length: u8,
    /// The length of the swap path for short token.
    pub short_token_swap_length: u8,
    /// The amount of glv tokens to burn.
    pub glv_token_amount: u64,
    /// Minimum acceptable final long token to receive.
    pub min_final_long_token_amount: u64,
    /// Minimum acceptable final short token to receive.
    pub min_final_short_token_amount: u64,
    /// Whether to unwrap native token when sending funds back.
    pub should_unwrap_native_token: bool,
}

impl ActionParams for CreateGlvWithdrawalParams {
    fn execution_lamports(&self) -> u64 {
        self.execution_lamports
    }
}

/// Operation for creating GLV withdrawal.
#[derive(TypedBuilder)]
pub(crate) struct CreateGlvWithdrawalOperation<'a, 'info> {
    glv_withdrawal: AccountLoader<'info, GlvWithdrawal>,
    market: AccountLoader<'info, Market>,
    store: AccountLoader<'info, Store>,
    owner: &'a AccountInfo<'info>,
    receiver: &'a AccountInfo<'info>,
    nonce: &'a NonceBytes,
    bump: u8,
    final_long_token: &'a Account<'info, TokenAccount>,
    final_short_token: &'a Account<'info, TokenAccount>,
    market_token: &'a Account<'info, TokenAccount>,
    glv_token: &'a InterfaceAccount<'info, token_interface::TokenAccount>,
    params: &'a CreateGlvWithdrawalParams,
    swap_paths: &'info [AccountInfo<'info>],
}

impl CreateGlvWithdrawalOperation<'_, '_> {
    /// Execute.
    ///
    /// # CHECK
    ///
    /// # Errors
    ///
    pub(crate) fn unchecked_execute(self) -> Result<()> {
        let (long_token, short_token) = self.validate_market_and_get_tokens()?;

        self.validate_params_excluding_swap()?;

        let id = self
            .market
            .load_mut()?
            .indexer_mut()
            .next_glv_withdrawal_id()?;

        let mut glv_withdrawal = self.glv_withdrawal.load_init()?;

        // Init header.
        glv_withdrawal.header.init(
            id,
            self.store.key(),
            self.market.key(),
            self.owner.key(),
            self.receiver.key(),
            *self.nonce,
            self.bump,
            self.params.execution_lamports,
            self.params.should_unwrap_native_token,
        )?;

        // Init tokens and token accounts.
        let tokens = &mut glv_withdrawal.tokens;
        tokens.glv_token.init_with_interface(self.glv_token);
        tokens.market_token.init(self.market_token);
        tokens.final_long_token.init(self.final_long_token);
        tokens.final_short_token.init(self.final_short_token);

        // Init params.
        let params = &mut glv_withdrawal.params;
        params.glv_token_amount = self.params.glv_token_amount;
        params.min_final_long_token_amount = self.params.min_final_long_token_amount;
        params.min_final_short_token_amount = self.params.min_final_short_token_amount;

        // Init swap paths.
        glv_withdrawal.swap.validate_and_init(
            &*self.market.load()?,
            self.params.long_token_swap_length,
            self.params.short_token_swap_length,
            self.swap_paths,
            &self.store.key(),
            (&long_token, &short_token),
            (&self.final_long_token.mint, &self.final_short_token.mint),
        )?;

        Ok(())
    }

    fn validate_market_and_get_tokens(&self) -> Result<(Pubkey, Pubkey)> {
        let market = self.market.load()?;
        let meta = market.validated_meta(&self.store.key())?;
        Ok((meta.long_token_mint, meta.short_token_mint))
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        let params = self.params;
        let amount = params.glv_token_amount;
        require!(amount != 0, CoreError::EmptyGlvWithdrawal);
        require_gte!(
            self.glv_token.amount,
            amount,
            CoreError::NotEnoughTokenAmount
        );

        ActionExt::validate_balance(&self.glv_withdrawal, params.execution_lamports)?;

        Ok(())
    }
}

/// Operation for executing a GLV withdrawal.
#[derive(TypedBuilder)]
pub(crate) struct ExecuteGlvWithdrawalOperation<'a, 'info> {
    glv_withdrawal: AccountLoader<'info, GlvWithdrawal>,
    token_program: AccountInfo<'info>,
    glv_token_program: AccountInfo<'info>,
    throw_on_execution_error: bool,
    store: AccountLoader<'info, Store>,
    glv: &'a AccountLoader<'info, Glv>,
    glv_token_mint: &'a mut InterfaceAccount<'info, token_interface::Mint>,
    glv_token_account: AccountInfo<'info>,
    market: AccountLoader<'info, Market>,
    market_token_mint: &'a mut Account<'info, Mint>,
    market_token_glv_vault: &'a Account<'info, TokenAccount>,
    market_token_withdrawal_vault: AccountInfo<'info>,
    markets: &'info [AccountInfo<'info>],
    market_tokens: &'info [AccountInfo<'info>],
    oracle: &'a Oracle,
    remaining_accounts: &'info [AccountInfo<'info>],
    #[builder(setter(into))]
    event_emitter: EventEmitter<'a, 'info>,
}

impl ExecuteGlvWithdrawalOperation<'_, '_> {
    /// Execute.
    ///
    /// # CHECK
    ///
    /// # Errors
    ///
    pub(crate) fn unchecked_execute(mut self) -> Result<Option<(u64, u64)>> {
        let throw_on_execution_error = self.throw_on_execution_error;
        match self.validate_oracle() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired) if !throw_on_execution_error => {
                msg!(
                    "GLV Withdrawal expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok(None);
            }
            Err(err) => {
                return Err(error!(err));
            }
        }

        let executed = match self.perform_glv_withdrawal() {
            Ok(amounts) => Some(amounts),
            Err(err) if !throw_on_execution_error => {
                msg!("Execute GLV withdrawal error: {}", err);
                None
            }
            Err(err) => return Err(err),
        };

        self.validate_after_execution()?;

        Ok(executed)
    }

    fn validate_oracle(&self) -> CoreResult<()> {
        self.oracle.validate_time(self)
    }

    fn validate_market(&self) -> Result<()> {
        self.market.load()?.validate(&self.store.key())?;
        Ok(())
    }

    fn validate_after_execution(&self) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        let glv = self.glv.load()?;
        let market_token = self.market_token_mint.key();
        let vault_balance = amount(&self.market_token_glv_vault.to_account_info())?;

        require_gte!(
            vault_balance,
            glv.market_config(&market_token)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance(),
            CoreError::Internal
        );

        Ok(())
    }

    #[inline(never)]
    fn perform_glv_withdrawal(&mut self) -> Result<(u64, u64)> {
        use gmsol_model::utils::market_token_amount_to_usd;

        self.validate_market()?;

        let withdrawal_signer = self.glv_withdrawal.load()?.signer();

        let (glv_token_amount, amounts) = {
            let withdrawal = self.glv_withdrawal.load()?;
            let glv_token_amount = withdrawal.params.glv_token_amount;
            let market_token_mint = self.market_token_mint.to_account_info();
            let market_token_decimals = self.market_token_mint.decimals;

            let mut market = RevertibleLiquidityMarketOperation::new(
                &self.store,
                self.oracle,
                &self.market,
                self.market_token_mint,
                self.token_program.clone(),
                Some(&withdrawal.swap),
                self.remaining_accounts,
                self.event_emitter,
            )?;

            let op = market.op()?;

            // Calculate market token amount to withdrawal.
            let market_token_amount = {
                let maximize_value = false;
                let glv_supply = self.glv_token_mint.supply;
                let glv_value = unchecked_get_glv_value(
                    &*self.glv.load()?,
                    self.oracle,
                    &op,
                    self.markets,
                    self.market_tokens,
                    maximize_value,
                )?;

                let market_token_value = market_token_amount_to_usd(
                    &(u128::from(glv_token_amount)),
                    &glv_value,
                    &(u128::from(glv_supply)),
                )
                .ok_or_else(|| error!(CoreError::FailedToCalculateGlvValueForMarket))?;

                msg!(
                    "[GLV] Calculating GM amount with glv_supply={}, glv_value={}, market_token_value={}",
                    glv_supply,
                    glv_value,
                    market_token_value,
                );

                let amount = get_market_token_amount_for_glv_value(
                    self.oracle,
                    op.market(),
                    market_token_value,
                    true,
                )?
                .try_into()
                .map_err(|_| error!(CoreError::TokenAmountOverflow))?;

                self.event_emitter.emit_cpi(&GlvPricing {
                    glv_token: self.glv_token_mint.key(),
                    market_token: market_token_mint.key(),
                    supply: glv_supply,
                    value_maximized: maximize_value,
                    value: glv_value,
                    input_amount: glv_token_amount,
                    input_value: market_token_value,
                    output_amount: amount,
                    kind: GlvPricingKind::Withdrawal,
                })?;

                amount
            };

            require_gte!(
                self.market_token_glv_vault.amount,
                market_token_amount,
                CoreError::NotEnoughTokenAmount,
            );

            let executed = {
                let mut params = WithdrawalActionParams::default();
                params.market_token_amount = market_token_amount;
                params.min_long_token_amount = withdrawal.params.min_final_long_token_amount;
                params.min_short_token_amount = withdrawal.params.min_final_short_token_amount;

                op.unchecked_withdraw(
                    &self.market_token_withdrawal_vault,
                    &params,
                    (
                        withdrawal.tokens.final_long_token(),
                        withdrawal.tokens.final_short_token(),
                    ),
                    None,
                )?
            };

            let amounts = executed.output;

            // Update market token balance.
            let next_market_token_balance = self
                .glv
                .load()?
                .market_config(market_token_mint.key)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance()
                .checked_sub(market_token_amount)
                .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;

            self.glv
                .load_mut()?
                .update_market_token_balance(market_token_mint.key, next_market_token_balance)?;

            // Transfer market tokens from the GLV vault to the withdrawal vault before the commitment.
            transfer_checked(
                CpiContext::new(
                    self.token_program.to_account_info(),
                    TransferChecked {
                        from: self.market_token_glv_vault.to_account_info(),
                        mint: market_token_mint,
                        to: self.market_token_withdrawal_vault.to_account_info(),
                        authority: self.glv.to_account_info(),
                    },
                )
                .with_signer(&[&self.glv.load()?.signer_seeds()]),
                market_token_amount,
                market_token_decimals,
            )
            .expect("failed to transfer market tokens");

            executed.commit();

            (glv_token_amount, amounts)
        };

        // Invertible operations after the commitment.
        {
            // Burn GLV tokens.
            self.burn_glv_tokens(&withdrawal_signer, glv_token_amount);
        }

        Ok(amounts)
    }

    /// Burn GLV tokens from the source account.
    ///
    /// # Panic
    /// This is an invertbile operation that will panic on error.
    fn burn_glv_tokens(&self, signer: &ActionSigner, glv_token_amount: u64) {
        use anchor_spl::token_interface::{burn, Burn};

        if glv_token_amount != 0 {
            let ctx = CpiContext::new(
                self.glv_token_program.to_account_info(),
                Burn {
                    mint: self.glv_token_mint.to_account_info(),
                    from: self.glv_token_account.clone(),
                    authority: self.glv_withdrawal.to_account_info(),
                },
            );
            burn(ctx.with_signer(&[&signer.as_seeds()]), glv_token_amount)
                .expect("failed to burn GLV tokens");
        }
    }
}

impl ValidateOracleTime for ExecuteGlvWithdrawalOperation<'_, '_> {
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>> {
        Ok(Some(
            self.glv_withdrawal
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header
                .updated_at,
        ))
    }

    fn oracle_updated_before(&self) -> CoreResult<Option<i64>> {
        let ts = self
            .store
            .load()
            .map_err(|_| CoreError::LoadAccountError)?
            .request_expiration_at(
                self.glv_withdrawal
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .header
                    .updated_at,
            )?;
        Ok(Some(ts))
    }

    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>> {
        Ok(Some(
            self.glv_withdrawal
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header
                .updated_at_slot,
        ))
    }
}

/// Get total GLV value.
///
/// # CHECK
/// Basically one must make sure that `glv_markets` and
/// `glv_market_tokens` are aligned.
///
/// # Errors
///
fn unchecked_get_glv_value<'info>(
    glv: &Glv,
    oracle: &Oracle,
    op: &Execute<'_, 'info>,
    glv_markets: &'info [AccountInfo<'info>],
    glv_market_tokens: &'info [AccountInfo<'info>],
    maximize: bool,
) -> Result<u128> {
    use crate::states::market::AsLiquidityMarket;

    let mut value = 0u128;

    let current_market = op.market();
    let swap_markets = op.swap_markets();

    let mut prices = oracle.market_prices(current_market)?;

    for (market, market_token) in glv_markets.iter().zip(glv_market_tokens) {
        let key = market_token.key();

        // Get the current balance of market tokens in the GLV vault.
        let balance = u128::from(
            glv.market_config(&key)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance(),
        );

        let value_for_market = if key == current_market.key() {
            let market = current_market;
            // Note that we should use the balance prior to the operation.
            get_glv_value_for_market_with_new_index_price(
                oracle,
                &mut prices,
                market,
                balance,
                maximize,
            )?
            .0
        } else if let Some(market) = swap_markets.get(&key) {
            let mint = Account::<Mint>::try_from(market_token)?;
            let market = AsLiquidityMarket::new(market, &mint);
            get_glv_value_for_market_with_new_index_price(
                oracle,
                &mut prices,
                &market,
                balance,
                maximize,
            )?
            .0
        } else {
            let market = AccountLoader::<Market>::try_from(market)?;
            let mint = Account::<Mint>::try_from(market_token)?;
            let market = market.load()?;
            let market = market.as_liquidity_market(&mint);
            get_glv_value_for_market_with_new_index_price(
                oracle,
                &mut prices,
                &market,
                balance,
                maximize,
            )?
            .0
        };

        value = value
            .checked_add(value_for_market)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;
    }

    Ok(value)
}

fn get_glv_value_for_market_with_new_index_price<M>(
    oracle: &Oracle,
    prices: &mut Prices<u128>,
    market: &M,
    balance: u128,
    maximize: bool,
) -> Result<(u128, i128, u128)>
where
    M: gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }, Num = u128, Signed = i128>,
    M: HasMarketMeta,
{
    let index_token_mint = market.market_meta().index_token_mint;
    prices.index_token_price = oracle
        .get_primary_price(&index_token_mint, true)
        .expect("must exist");

    get_glv_value_for_market(prices, market, balance, maximize)
}

fn get_glv_value_for_market<M>(
    prices: &Prices<u128>,
    market: &M,
    balance: u128,
    maximize: bool,
) -> Result<(u128, i128, u128)>
where
    M: gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }, Num = u128, Signed = i128>,
{
    use gmsol_model::{utils, LiquidityMarketExt};

    let value = market
        .pool_value(prices, PnlFactorKind::MaxAfterDeposit, maximize)
        .map_err(ModelError::from)?;

    let supply = market.total_supply();

    if balance == 0 {
        return Ok((0, value, supply));
    }

    if value.is_negative() {
        return err!(CoreError::GlvNegativeMarketPoolValue);
    }

    let glv_value = utils::market_token_amount_to_usd(&balance, &value.unsigned_abs(), &supply)
        .ok_or_else(|| error!(CoreError::FailedToCalculateGlvValueForMarket))?;

    Ok((glv_value, value, supply))
}

fn get_market_token_amount_for_glv_value<M>(
    oracle: &Oracle,
    market: &M,
    glv_value: u128,
    maximize: bool,
) -> Result<u128>
where
    M: gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }, Num = u128, Signed = i128>,
    M: HasMarketMeta,
{
    use gmsol_model::{utils, LiquidityMarketExt};

    let prices = oracle.market_prices(market).expect("must exist");

    let value = market
        .pool_value(&prices, PnlFactorKind::MaxAfterWithdrawal, maximize)
        .map_err(ModelError::from)?;

    if value.is_negative() {
        return err!(CoreError::GlvNegativeMarketPoolValue);
    }

    let supply = market.total_supply();

    let market_token_amount = utils::usd_to_market_token_amount(
        glv_value,
        value.unsigned_abs(),
        supply,
        constants::MARKET_USD_TO_AMOUNT_DIVISOR,
    )
    .ok_or_else(|| error!(CoreError::FailedTOCalculateMarketTokenAmountToBurn))?;

    Ok(market_token_amount)
}

/// Operation for executing a GLV withdrawal.
#[derive(TypedBuilder)]
pub(crate) struct ExecuteGlvShiftOperation<'a, 'info> {
    glv_shift: &'a AccountLoader<'info, GlvShift>,
    token_program: AccountInfo<'info>,
    throw_on_execution_error: bool,
    store: &'a AccountLoader<'info, Store>,
    glv: &'a AccountLoader<'info, Glv>,
    from_market: &'a AccountLoader<'info, Market>,
    from_market_token_mint: &'a mut Account<'info, Mint>,
    from_market_token_glv_vault: &'a Account<'info, TokenAccount>,
    from_market_token_withdrawal_vault: AccountInfo<'info>,
    to_market: &'a AccountLoader<'info, Market>,
    to_market_token_mint: &'a mut Account<'info, Mint>,
    to_market_token_glv_vault: AccountInfo<'info>,
    oracle: &'a Oracle,
    #[builder(setter(into))]
    event_emitter: EventEmitter<'a, 'info>,
}

impl ExecuteGlvShiftOperation<'_, '_> {
    /// Execute.
    ///
    /// # CHECK
    ///
    /// # Errors
    ///
    pub(crate) fn unchecked_execute(mut self) -> Result<bool> {
        let throw_on_execution_error = self.throw_on_execution_error;
        match self.validate_oracle() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired) if !throw_on_execution_error => {
                msg!(
                    "GLV Shift expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok(false);
            }
            Err(err) => {
                return Err(error!(err));
            }
        }

        let executed = match self.perform_glv_shift() {
            Ok(()) => true,
            Err(err) if !throw_on_execution_error => {
                msg!("Execute GLV shift error: {}", err);
                false
            }
            Err(err) => return Err(err),
        };

        self.validate_after_execution()?;

        Ok(executed)
    }

    fn validate_oracle(&self) -> CoreResult<()> {
        self.oracle.validate_time(self)
    }

    fn validate_before_execution(&self) -> Result<()> {
        self.glv.load()?.validate_shift_interval()?;

        require!(
            self.from_market.key() != self.to_market.key(),
            CoreError::Internal
        );

        let from_market = self.from_market.load()?;
        let to_market = self.to_market.load()?;

        from_market.validate(&self.store.key())?;
        to_market.validate(&self.store.key())?;

        from_market.validate_shiftable(&to_market)?;

        let shift = self.glv_shift.load()?;

        // Validate the vault has enough from market tokens.
        let amount = Borrow::<Shift>::borrow(&*shift)
            .params
            .from_market_token_amount;
        require_gte!(
            self.from_market_token_glv_vault.amount,
            amount,
            CoreError::NotEnoughTokenAmount
        );

        Ok(())
    }

    fn validate_after_execution(&self) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        let glv = self.glv.load()?;

        let from_market_token = self.from_market_token_mint.key();
        let from_market_token_vault_balance =
            amount(&self.from_market_token_glv_vault.to_account_info())?;
        require_gte!(
            from_market_token_vault_balance,
            glv.market_config(&from_market_token)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance(),
            CoreError::Internal
        );

        let to_market_token = self.to_market_token_mint.key();
        let to_market_token_vault_balance =
            amount(&self.to_market_token_glv_vault.to_account_info())?;
        require_gte!(
            to_market_token_vault_balance,
            glv.market_config(&to_market_token)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance(),
            CoreError::Internal
        );

        Ok(())
    }

    #[inline(never)]
    fn perform_glv_shift(&mut self) -> Result<()> {
        self.validate_before_execution()?;

        let from_market_token_address = self.from_market_token_mint.key();
        let to_market_token_address = self.to_market_token_mint.key();

        let from_market_token_mint = self.from_market_token_mint.to_account_info();
        let from_market_token_decimals = self.from_market_token_mint.decimals;
        let glv_shift = self.glv_shift.load()?;
        let shift = Borrow::<Shift>::borrow(&*glv_shift);

        let mut from_market = RevertibleLiquidityMarketOperation::new(
            self.store,
            self.oracle,
            self.from_market,
            self.from_market_token_mint,
            self.token_program.clone(),
            None,
            &[],
            self.event_emitter,
        )?;

        let mut to_market = RevertibleLiquidityMarketOperation::new(
            self.store,
            self.oracle,
            self.to_market,
            self.to_market_token_mint,
            self.token_program.clone(),
            None,
            &[],
            self.event_emitter,
        )?;

        let from_market = from_market.op()?;
        let to_market = to_market.op()?;

        let (from_market, to_market, received) = from_market.unchecked_shift(
            to_market,
            &shift.header().receiver(),
            &shift.params,
            &self.from_market_token_withdrawal_vault,
            &self.to_market_token_glv_vault,
        )?;

        // Validate to market token balance.
        let next_to_market_token_balance = {
            let (_, market_pool_value, market_token_supply) = {
                let mut prices = self.oracle.market_prices(to_market.market())?;
                get_glv_value_for_market_with_new_index_price(
                    self.oracle,
                    &mut prices,
                    to_market.market(),
                    0,
                    true,
                )?
            };
            let current_balance = self
                .glv
                .load()?
                .market_config(&to_market_token_address)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance();
            let new_balance = current_balance
                .checked_add(received)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            self.glv.load()?.validate_market_token_balance(
                &to_market.market().market_meta().market_token_mint,
                new_balance,
                &market_pool_value,
                &market_token_supply,
            )?;

            new_balance
        };

        // Validate max price impact and min shift value.
        {
            let mut prices = self.oracle.market_prices(from_market.market())?;

            let (from_market_token_value, _, _) = get_glv_value_for_market_with_new_index_price(
                self.oracle,
                &mut prices,
                from_market.market(),
                shift.params.from_market_token_amount().into(),
                true,
            )?;

            self.glv
                .load()?
                .validate_shift_value(from_market_token_value)?;

            let (to_market_token_value, _, _) = get_glv_value_for_market_with_new_index_price(
                self.oracle,
                &mut prices,
                to_market.market(),
                received.into(),
                true,
            )?;

            self.glv
                .load()?
                .validate_shift_price_impact(from_market_token_value, to_market_token_value)?;
        }

        // Transfer market tokens from the GLV vault to the withdrawal vault before the commitment.
        let next_from_market_token_balance = {
            let glv = self.glv.load()?;
            let seeds = glv.signer_seeds();

            let amount = shift.params.from_market_token_amount;
            let next_from_market_token_balance = glv
                .market_config(&from_market_token_address)
                .ok_or_else(|| error!(CoreError::NotFound))?
                .balance()
                .checked_sub(amount)
                .ok_or_else(|| error!(CoreError::NotEnoughTokenAmount))?;

            transfer_checked(
                CpiContext::new(
                    self.token_program.to_account_info(),
                    TransferChecked {
                        from: self.from_market_token_glv_vault.to_account_info(),
                        mint: from_market_token_mint,
                        to: self.from_market_token_withdrawal_vault.to_account_info(),
                        authority: self.glv.to_account_info(),
                    },
                )
                .with_signer(&[&seeds]),
                amount,
                from_market_token_decimals,
            )
            .expect("failed to transfer from market tokens");

            next_from_market_token_balance
        };

        // Commit the changes.
        from_market.commit();
        to_market.commit();

        // Invertible operations after the commitment.
        {
            let mut glv = self.glv.load_mut().expect("must success");
            glv.update_shift_last_executed_ts()
                .expect("failed to update shift last executed ts");
            glv.update_market_token_balance(
                &from_market_token_address,
                next_from_market_token_balance,
            )
            .expect("failed to update from market token balance");
            glv.update_market_token_balance(&to_market_token_address, next_to_market_token_balance)
                .expect("failed to update from market token balance");
        }

        Ok(())
    }
}

impl ValidateOracleTime for ExecuteGlvShiftOperation<'_, '_> {
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>> {
        Ok(Some(
            self.glv_shift
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header()
                .updated_at,
        ))
    }

    fn oracle_updated_before(&self) -> CoreResult<Option<i64>> {
        let ts = self
            .store
            .load()
            .map_err(|_| CoreError::LoadAccountError)?
            .request_expiration_at(
                self.glv_shift
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .header()
                    .updated_at,
            )?;
        Ok(Some(ts))
    }

    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>> {
        Ok(Some(
            self.glv_shift
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header()
                .updated_at_slot,
        ))
    }
}
