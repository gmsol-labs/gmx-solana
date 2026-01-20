use std::num::NonZeroU64;

use bytemuck::Zeroable;

use crate::gmsol_store::{
    accounts::{Glv, GtExchange, Market, Position, ReferralCodeV2, Store, VirtualInventory},
    types::{ActionHeader, EventPositionState, Pool, PositionState, UpdateOrderParams},
};

/// Referral Code Bytes.
pub type ReferralCodeBytes = [u8; 8];

impl Default for Market {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for VirtualInventory {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Pool {
    /// Returns whether the pool is a pure pool.
    pub fn is_pure(&self) -> bool {
        !matches!(self.is_pure, 0)
    }
}

impl Default for Glv {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for Position {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl AsRef<Position> for Position {
    fn as_ref(&self) -> &Position {
        self
    }
}

impl Default for ActionHeader {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Default for GtExchange {
    fn default() -> Self {
        Zeroable::zeroed()
    }
}

impl Store {
    /// Get claimable time window size.
    pub fn claimable_time_window(&self) -> crate::Result<NonZeroU64> {
        NonZeroU64::new(self.amount.claimable_time_window)
            .ok_or_else(|| crate::Error::custom("claimable time window cannot be zero"))
    }

    /// Get claimable time window index for the given timestamp.
    pub fn claimable_time_window_index(&self, timestamp: i64) -> crate::Result<i64> {
        let window: i64 = self
            .claimable_time_window()?
            .get()
            .try_into()
            .map_err(crate::Error::custom)?;
        Ok(timestamp / window)
    }

    /// Get claimable time key for the given timestamp.
    pub fn claimable_time_key(&self, timestamp: i64) -> crate::Result<[u8; 8]> {
        let index = self.claimable_time_window_index(timestamp)?;
        Ok(index.to_le_bytes())
    }
}

#[cfg(feature = "model")]
impl Store {
    /// Get order fee discount factor.
    pub fn order_fee_discount_factor(&self, rank: u8, is_referred: bool) -> crate::Result<u128> {
        use crate::constants::{MARKET_DECIMALS, MARKET_USD_UNIT};
        use gmsol_model::utils::apply_factor;

        if (rank as u64) > self.gt.max_rank {
            return Err(crate::Error::custom(format!(
                "rank {rank} exceeds max_rank {}",
                self.gt.max_rank
            )));
        }

        let discount_factor_for_rank = self.gt.order_fee_discount_factors[rank as usize];

        if is_referred {
            let discount_factor_for_referred = self.factor.order_fee_discount_for_referred_user;

            let complement_discount_factor_for_referred = MARKET_USD_UNIT
                .checked_sub(discount_factor_for_referred)
                .ok_or_else(|| crate::Error::custom("complement calculation overflow"))?;

            let discount_factor = apply_factor::<_, { MARKET_DECIMALS }>(
                &discount_factor_for_rank,
                &complement_discount_factor_for_referred,
            )
            .and_then(|factor| discount_factor_for_referred.checked_add(factor))
            .ok_or_else(|| crate::Error::custom("discount factor calculation overflow"))?;

            Ok(discount_factor)
        } else {
            Ok(discount_factor_for_rank)
        }
    }
}

impl ReferralCodeV2 {
    /// The length of referral code.
    pub const LEN: usize = std::mem::size_of::<ReferralCodeBytes>();

    /// Decode the given code string to code bytes.
    pub fn decode(code: &str) -> crate::Result<ReferralCodeBytes> {
        if code.is_empty() {
            return Err(crate::Error::custom("empty code is not supported"));
        }
        let code = bs58::decode(code)
            .into_vec()
            .map_err(crate::Error::custom)?;
        if code.len() > Self::LEN {
            return Err(crate::Error::custom("the code is too long"));
        }
        let padding = Self::LEN - code.len();
        let mut code_bytes = ReferralCodeBytes::default();
        code_bytes[padding..].copy_from_slice(&code);

        Ok(code_bytes)
    }

    /// Encode the given code to code string.
    pub fn encode(code: &ReferralCodeBytes, skip_leading_ones: bool) -> String {
        let code = bs58::encode(code).into_string();
        if skip_leading_ones {
            code.trim_start_matches('1').to_owned()
        } else {
            code
        }
    }
}

impl From<EventPositionState> for PositionState {
    fn from(event: EventPositionState) -> Self {
        let EventPositionState {
            trade_id,
            increased_at,
            updated_at_slot,
            decreased_at,
            size_in_tokens,
            collateral_amount,
            size_in_usd,
            borrowing_factor,
            funding_fee_amount_per_size,
            long_token_claimable_funding_amount_per_size,
            short_token_claimable_funding_amount_per_size,
            reserved,
        } = event;

        Self {
            trade_id,
            increased_at,
            updated_at_slot,
            decreased_at,
            size_in_tokens,
            collateral_amount,
            size_in_usd,
            borrowing_factor,
            funding_fee_amount_per_size,
            long_token_claimable_funding_amount_per_size,
            short_token_claimable_funding_amount_per_size,
            reserved,
        }
    }
}

impl UpdateOrderParams {
    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.size_delta_value.is_none()
            && self.acceptable_price.is_none()
            && self.trigger_price.is_none()
            && self.min_output.is_none()
            && self.valid_from_ts.is_none()
    }
}

#[cfg(feature = "gmsol-model")]
mod model {
    use gmsol_model::ClockKind;

    use crate::gmsol_store::types::Clocks;

    impl Clocks {
        /// Get clock value by [`ClockKind`].
        pub fn get(&self, kind: ClockKind) -> Option<i64> {
            let clock = match kind {
                ClockKind::PriceImpactDistribution => self.price_impact_distribution,
                ClockKind::Borrowing => self.borrowing,
                ClockKind::Funding => self.funding,
                ClockKind::AdlForLong => self.adl_for_long,
                ClockKind::AdlForShort => self.adl_for_short,
                _ => return None,
            };
            Some(clock)
        }
    }
}

#[cfg(feature = "gmsol-utils")]
mod utils {
    use anchor_lang::prelude::Pubkey;
    use gmsol_utils::{
        action::{ActionCallbackKind, ActionFlag, ActionState, MAX_ACTION_FLAGS},
        fixed_str::bytes_to_fixed_str,
        glv::{GlvMarketFlag, MAX_GLV_MARKET_FLAGS},
        impl_fixed_map, impl_flags,
        market::{
            self, HasMarketMeta, MarketConfigFactor, MarketConfigFlag, MarketConfigKey, MarketFlag,
            VirtualInventoryFlag, MAX_MARKET_CONFIG_FACTORS, MAX_MARKET_CONFIG_FLAGS,
            MAX_MARKET_FLAGS, MAX_VIRTUAL_INVENTORY_FLAGS,
        },
        order::{self, OrderFlag, PositionKind, TradeFlag, TradeFlagContainer, MAX_ORDER_FLAGS},
        pubkey::{self, optional_address},
        swap::{self, HasSwapParams},
        token_config::{self, TokensCollector},
    };

    use crate::gmsol_store::{
        accounts::{Glv, Market, Position},
        events::TradeEvent,
        types::{
            ActionFlagContainer, ActionHeader, GlvMarketConfig, GlvMarketFlagContainer, GlvMarkets,
            GlvMarketsEntry, MarketConfig, MarketConfigFactorContainer, MarketConfigFlagContainer,
            MarketFlagContainer, MarketMeta, Members, MembersEntry, OrderActionParams,
            OrderFlagContainer, OrderKind, RoleMap, RoleMapEntry, RoleMetadata, RoleStore,
            SwapActionParams, TokenAndAccount, Tokens, TokensEntry, UpdateTokenConfigParams,
            VirtualInventoryFlagContainer,
        },
    };

    const MAX_TOKENS: usize = 256;
    const MAX_ALLOWED_NUMBER_OF_MARKETS: usize = 96;
    const MAX_ROLES: usize = 32;
    const MAX_MEMBERS: usize = 64;

    impl_fixed_map!(RoleMap, RoleMetadata, MAX_ROLES);

    impl_fixed_map!(Members, Pubkey, pubkey::to_bytes, u32, MAX_MEMBERS);

    impl_fixed_map!(Tokens, Pubkey, pubkey::to_bytes, u8, MAX_TOKENS);
    impl_fixed_map!(
        GlvMarkets,
        Pubkey,
        pubkey::to_bytes,
        GlvMarketConfig,
        MAX_ALLOWED_NUMBER_OF_MARKETS
    );

    impl_flags!(ActionFlag, MAX_ACTION_FLAGS, u8);
    impl_flags!(MarketFlag, MAX_MARKET_FLAGS, u8);
    impl_flags!(GlvMarketFlag, MAX_GLV_MARKET_FLAGS, u8);
    impl_flags!(VirtualInventoryFlag, MAX_VIRTUAL_INVENTORY_FLAGS, u8);
    impl_flags!(MarketConfigFlag, MAX_MARKET_CONFIG_FLAGS, u128);
    impl_flags!(MarketConfigFactor, MAX_MARKET_CONFIG_FACTORS, u128);
    impl_flags!(OrderFlag, MAX_ORDER_FLAGS, u8);

    impl From<SwapActionParams> for swap::SwapActionParams {
        fn from(params: SwapActionParams) -> Self {
            let SwapActionParams {
                primary_length,
                secondary_length,
                num_tokens,
                padding_0,
                current_market_token,
                paths,
                tokens,
            } = params;
            Self {
                primary_length,
                secondary_length,
                num_tokens,
                padding_0,
                current_market_token,
                paths,
                tokens,
            }
        }
    }

    impl From<MarketMeta> for market::MarketMeta {
        fn from(meta: MarketMeta) -> Self {
            let MarketMeta {
                market_token_mint,
                index_token_mint,
                long_token_mint,
                short_token_mint,
            } = meta;
            Self {
                market_token_mint,
                index_token_mint,
                long_token_mint,
                short_token_mint,
            }
        }
    }

    impl Market {
        /// Get name.
        pub fn name(&self) -> crate::Result<&str> {
            bytes_to_fixed_str(&self.name).map_err(crate::Error::custom)
        }
    }

    impl HasMarketMeta for Market {
        fn market_meta(&self) -> &market::MarketMeta {
            bytemuck::cast_ref(&self.meta)
        }
    }

    impl MarketConfig {
        /// Get config by [`MarketConfigKey`].
        pub fn get(&self, key: MarketConfigKey) -> Option<&u128> {
            let value = match key {
                MarketConfigKey::SwapImpactExponent => &self.swap_impact_exponent,
                MarketConfigKey::SwapImpactPositiveFactor => &self.swap_impact_positive_factor,
                MarketConfigKey::SwapImpactNegativeFactor => &self.swap_impact_negative_factor,
                MarketConfigKey::SwapFeeReceiverFactor => &self.swap_fee_receiver_factor,
                MarketConfigKey::SwapFeeFactorForPositiveImpact => {
                    &self.swap_fee_factor_for_positive_impact
                }
                MarketConfigKey::SwapFeeFactorForNegativeImpact => {
                    &self.swap_fee_factor_for_negative_impact
                }
                MarketConfigKey::MinPositionSizeUsd => &self.min_position_size_usd,
                MarketConfigKey::MinCollateralValue => &self.min_collateral_value,
                MarketConfigKey::MinCollateralFactor => &self.min_collateral_factor,
                MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForLong => {
                    &self.min_collateral_factor_for_open_interest_multiplier_for_long
                }
                MarketConfigKey::MinCollateralFactorForOpenInterestMultiplierForShort => {
                    &self.min_collateral_factor_for_open_interest_multiplier_for_short
                }
                MarketConfigKey::MaxPositivePositionImpactFactor => {
                    &self.max_positive_position_impact_factor
                }
                MarketConfigKey::MaxNegativePositionImpactFactor => {
                    &self.max_negative_position_impact_factor
                }
                MarketConfigKey::MaxPositionImpactFactorForLiquidations => {
                    &self.max_position_impact_factor_for_liquidations
                }
                MarketConfigKey::PositionImpactExponent => &self.position_impact_exponent,
                MarketConfigKey::PositionImpactPositiveFactor => {
                    &self.position_impact_positive_factor
                }
                MarketConfigKey::PositionImpactNegativeFactor => {
                    &self.position_impact_negative_factor
                }
                MarketConfigKey::OrderFeeReceiverFactor => &self.order_fee_receiver_factor,
                MarketConfigKey::OrderFeeFactorForPositiveImpact => {
                    &self.order_fee_factor_for_positive_impact
                }
                MarketConfigKey::OrderFeeFactorForNegativeImpact => {
                    &self.order_fee_factor_for_negative_impact
                }
                MarketConfigKey::LiquidationFeeReceiverFactor => {
                    &self.liquidation_fee_receiver_factor
                }
                MarketConfigKey::LiquidationFeeFactor => &self.liquidation_fee_factor,
                MarketConfigKey::PositionImpactDistributeFactor => {
                    &self.position_impact_distribute_factor
                }
                MarketConfigKey::MinPositionImpactPoolAmount => {
                    &self.min_position_impact_pool_amount
                }
                MarketConfigKey::BorrowingFeeReceiverFactor => &self.borrowing_fee_receiver_factor,
                MarketConfigKey::BorrowingFeeFactorForLong => &self.borrowing_fee_factor_for_long,
                MarketConfigKey::BorrowingFeeFactorForShort => &self.borrowing_fee_factor_for_short,
                MarketConfigKey::BorrowingFeeExponentForLong => {
                    &self.borrowing_fee_exponent_for_long
                }
                MarketConfigKey::BorrowingFeeExponentForShort => {
                    &self.borrowing_fee_exponent_for_short
                }
                MarketConfigKey::BorrowingFeeOptimalUsageFactorForLong => {
                    &self.borrowing_fee_optimal_usage_factor_for_long
                }
                MarketConfigKey::BorrowingFeeOptimalUsageFactorForShort => {
                    &self.borrowing_fee_optimal_usage_factor_for_short
                }
                MarketConfigKey::BorrowingFeeBaseFactorForLong => {
                    &self.borrowing_fee_base_factor_for_long
                }
                MarketConfigKey::BorrowingFeeBaseFactorForShort => {
                    &self.borrowing_fee_base_factor_for_short
                }
                MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForLong => {
                    &self.borrowing_fee_above_optimal_usage_factor_for_long
                }
                MarketConfigKey::BorrowingFeeAboveOptimalUsageFactorForShort => {
                    &self.borrowing_fee_above_optimal_usage_factor_for_short
                }
                MarketConfigKey::FundingFeeExponent => &self.funding_fee_exponent,
                MarketConfigKey::FundingFeeFactor => &self.funding_fee_factor,
                MarketConfigKey::FundingFeeMaxFactorPerSecond => {
                    &self.funding_fee_max_factor_per_second
                }
                MarketConfigKey::FundingFeeMinFactorPerSecond => {
                    &self.funding_fee_min_factor_per_second
                }
                MarketConfigKey::FundingFeeIncreaseFactorPerSecond => {
                    &self.funding_fee_increase_factor_per_second
                }
                MarketConfigKey::FundingFeeDecreaseFactorPerSecond => {
                    &self.funding_fee_decrease_factor_per_second
                }
                MarketConfigKey::FundingFeeThresholdForStableFunding => {
                    &self.funding_fee_threshold_for_stable_funding
                }
                MarketConfigKey::FundingFeeThresholdForDecreaseFunding => {
                    &self.funding_fee_threshold_for_decrease_funding
                }
                MarketConfigKey::ReserveFactor => &self.reserve_factor,
                MarketConfigKey::OpenInterestReserveFactor => &self.open_interest_reserve_factor,
                MarketConfigKey::MaxPnlFactorForLongDeposit => {
                    &self.max_pnl_factor_for_long_deposit
                }
                MarketConfigKey::MaxPnlFactorForShortDeposit => {
                    &self.max_pnl_factor_for_short_deposit
                }
                MarketConfigKey::MaxPnlFactorForLongWithdrawal => {
                    &self.max_pnl_factor_for_long_withdrawal
                }
                MarketConfigKey::MaxPnlFactorForShortWithdrawal => {
                    &self.max_pnl_factor_for_short_withdrawal
                }
                MarketConfigKey::MaxPnlFactorForLongTrader => &self.max_pnl_factor_for_long_trader,
                MarketConfigKey::MaxPnlFactorForShortTrader => {
                    &self.max_pnl_factor_for_short_trader
                }
                MarketConfigKey::MaxPnlFactorForLongAdl => &self.max_pnl_factor_for_long_adl,
                MarketConfigKey::MaxPnlFactorForShortAdl => &self.max_pnl_factor_for_short_adl,
                MarketConfigKey::MinPnlFactorAfterLongAdl => &self.min_pnl_factor_after_long_adl,
                MarketConfigKey::MinPnlFactorAfterShortAdl => &self.min_pnl_factor_after_short_adl,
                MarketConfigKey::MaxPoolAmountForLongToken => &self.max_pool_amount_for_long_token,
                MarketConfigKey::MaxPoolAmountForShortToken => {
                    &self.max_pool_amount_for_short_token
                }
                MarketConfigKey::MaxPoolValueForDepositForLongToken => {
                    &self.max_pool_value_for_deposit_for_long_token
                }
                MarketConfigKey::MaxPoolValueForDepositForShortToken => {
                    &self.max_pool_value_for_deposit_for_short_token
                }
                MarketConfigKey::MaxOpenInterestForLong => &self.max_open_interest_for_long,
                MarketConfigKey::MaxOpenInterestForShort => &self.max_open_interest_for_short,
                MarketConfigKey::MinTokensForFirstDeposit => &self.min_tokens_for_first_deposit,
                MarketConfigKey::MinCollateralFactorForLiquidation => {
                    &self.min_collateral_factor_for_liquidation
                }
                MarketConfigKey::MarketClosedMinCollateralFactorForLiquidation => {
                    &self.market_closed_min_collateral_factor_for_liquidation
                }
                MarketConfigKey::MarketClosedBorrowingFeeBaseFactor => {
                    &self.market_closed_borrowing_fee_base_factor
                }
                MarketConfigKey::MarketClosedBorrowingFeeAboveOptimalUsageFactor => {
                    &self.market_closed_borrowing_fee_above_optimal_usage_factor
                }
                _ => return None,
            };
            Some(value)
        }
    }

    impl TokenAndAccount {
        /// Get token.
        pub fn token(&self) -> Option<Pubkey> {
            optional_address(&self.token).copied()
        }

        /// Get account.
        pub fn account(&self) -> Option<Pubkey> {
            optional_address(&self.account).copied()
        }

        /// Get token and account.
        pub fn token_and_account(&self) -> Option<(Pubkey, Pubkey)> {
            let token = self.token()?;
            let account = self.account()?;
            Some((token, account))
        }
    }

    impl From<OrderKind> for order::OrderKind {
        fn from(value: OrderKind) -> Self {
            match value {
                OrderKind::Liquidation => Self::Liquidation,
                OrderKind::AutoDeleveraging => Self::AutoDeleveraging,
                OrderKind::MarketSwap => Self::MarketSwap,
                OrderKind::MarketIncrease => Self::MarketIncrease,
                OrderKind::MarketDecrease => Self::MarketDecrease,
                OrderKind::LimitSwap => Self::LimitSwap,
                OrderKind::LimitIncrease => Self::LimitIncrease,
                OrderKind::LimitDecrease => Self::LimitDecrease,
                OrderKind::StopLossDecrease => Self::StopLossDecrease,
            }
        }
    }

    impl TryFrom<order::OrderKind> for OrderKind {
        type Error = crate::Error;

        fn try_from(value: order::OrderKind) -> Result<Self, Self::Error> {
            match value {
                order::OrderKind::Liquidation => Ok(Self::Liquidation),
                order::OrderKind::AutoDeleveraging => Ok(Self::AutoDeleveraging),
                order::OrderKind::MarketSwap => Ok(Self::MarketSwap),
                order::OrderKind::MarketIncrease => Ok(Self::MarketIncrease),
                order::OrderKind::MarketDecrease => Ok(Self::MarketDecrease),
                order::OrderKind::LimitSwap => Ok(Self::LimitSwap),
                order::OrderKind::LimitIncrease => Ok(Self::LimitIncrease),
                order::OrderKind::LimitDecrease => Ok(Self::LimitDecrease),
                order::OrderKind::StopLossDecrease => Ok(Self::StopLossDecrease),
                kind => Err(crate::Error::custom(format!(
                    "unsupported order kind: {kind}"
                ))),
            }
        }
    }

    impl OrderActionParams {
        /// Get order side.
        pub fn side(&self) -> crate::Result<order::OrderSide> {
            self.side.try_into().map_err(crate::Error::custom)
        }

        /// Get order kind.
        pub fn kind(&self) -> crate::Result<order::OrderKind> {
            self.kind.try_into().map_err(crate::Error::custom)
        }

        /// Get position.
        pub fn position(&self) -> Option<&Pubkey> {
            optional_address(&self.position)
        }

        /// Get decrease position swap type.
        #[cfg(feature = "model")]
        pub fn decrease_position_swap_type(
            &self,
        ) -> crate::Result<gmsol_model::action::decrease_position::DecreasePositionSwapType>
        {
            let ty = self
                .decrease_position_swap_type
                .try_into()
                .map_err(|_| crate::Error::custom("unknown decrease position swap type"))?;
            Ok(ty)
        }
    }

    impl Position {
        /// Get position kind.
        pub fn kind(&self) -> crate::Result<PositionKind> {
            self.kind.try_into().map_err(crate::Error::custom)
        }
    }

    impl Glv {
        /// Get all market tokens.
        pub fn market_tokens(&self) -> impl Iterator<Item = Pubkey> + '_ {
            self.markets
                .entries()
                .map(|(key, _)| Pubkey::new_from_array(*key))
        }

        /// Get [`GlvMarketConfig`] for the given market.
        pub fn market_config(&self, market_token: &Pubkey) -> Option<&GlvMarketConfig> {
            self.markets.get(market_token)
        }

        /// Get the total number of markets.
        pub fn num_markets(&self) -> usize {
            self.markets.len()
        }

        /// Create a new [`TokensCollector`].
        pub fn tokens_collector(&self, action: Option<&impl HasSwapParams>) -> TokensCollector {
            let mut collector = TokensCollector::new(action, self.num_markets());
            if action.is_none() {
                collector.insert_token(&self.long_token);
                collector.insert_token(&self.short_token);
            }
            collector
        }
    }

    impl From<token_config::UpdateTokenConfigParams> for UpdateTokenConfigParams {
        fn from(params: token_config::UpdateTokenConfigParams) -> Self {
            let token_config::UpdateTokenConfigParams {
                heartbeat_duration,
                precision,
                feeds,
                timestamp_adjustments,
                expected_provider,
            } = params;
            Self {
                heartbeat_duration,
                precision,
                feeds,
                timestamp_adjustments,
                expected_provider,
            }
        }
    }

    impl ActionHeader {
        /// Get action state.
        pub fn action_state(&self) -> crate::Result<ActionState> {
            ActionState::try_from(self.action_state)
                .map_err(|_| crate::Error::custom("unknown action state"))
        }

        /// Get callback kind.
        pub fn callback_kind(&self) -> crate::Result<ActionCallbackKind> {
            ActionCallbackKind::try_from(self.callback_kind)
                .map_err(|_| crate::Error::custom("unknown callback kind"))
        }
    }

    impl TradeEvent {
        /// Get trade data flag.
        pub fn get_flag(&self, flag: TradeFlag) -> bool {
            let map = TradeFlagContainer::from_value(self.flags);
            map.get_flag(flag)
        }

        /// Return whether the position side is long.
        pub fn is_long(&self) -> bool {
            self.get_flag(TradeFlag::IsLong)
        }

        /// Return whether the collateral side is long.
        pub fn is_collateral_long(&self) -> bool {
            self.get_flag(TradeFlag::IsCollateralLong)
        }

        /// Create position from this event.
        pub fn to_position(&self, meta: &impl HasMarketMeta) -> Position {
            let mut position = Position::default();

            let kind = if self.is_long() {
                PositionKind::Long
            } else {
                PositionKind::Short
            };

            let collateral_token = if self.is_collateral_long() {
                meta.market_meta().long_token_mint
            } else {
                meta.market_meta().short_token_mint
            };

            position.kind = kind as u8;
            // Note: there's no need to provide a correct bump here for now.
            position.bump = 0;
            position.store = self.store;
            position.owner = self.user;
            position.market_token = self.market_token;
            position.collateral_token = collateral_token;
            position.state = self.after.into();
            position
        }
    }

    impl RoleMetadata {
        /// A `u8` value indicates that this role is enabled.
        pub const ROLE_ENABLED: u8 = u8::MAX;

        /// Get the name of this role.
        pub fn name(&self) -> crate::Result<&str> {
            bytes_to_fixed_str(&self.name).map_err(crate::Error::custom)
        }

        /// Is enabled.
        pub fn is_enabled(&self) -> bool {
            self.enabled == Self::ROLE_ENABLED
        }
    }

    impl RoleStore {
        fn enabled_role_index(&self, role: &str) -> crate::Result<Option<u8>> {
            if let Some(metadata) = self.roles.get(role) {
                if metadata.name()? != role {
                    return Err(crate::Error::custom("invalid role store"));
                }
                if !metadata.is_enabled() {
                    return Err(crate::Error::custom("the given role is disabled"));
                }
                Ok(Some(metadata.index))
            } else {
                Ok(None)
            }
        }

        /// Returns whether the address has the give role.
        pub fn has_role(&self, authority: &Pubkey, role: &str) -> crate::Result<bool> {
            use gmsol_utils::bitmaps::Bitmap;
            type RoleBitmap = Bitmap<MAX_ROLES>;

            let value = self
                .members
                .get(authority)
                .ok_or_else(|| crate::Error::custom("not a member"))?;
            let index = self
                .enabled_role_index(role)?
                .ok_or_else(|| crate::Error::custom("no such role"))?;
            let bitmap = RoleBitmap::from_value(*value);
            Ok(bitmap.get(index as usize))
        }

        /// Returns all members.
        pub fn members(&self) -> impl Iterator<Item = Pubkey> + '_ {
            self.members
                .entries()
                .map(|(key, _)| Pubkey::new_from_array(*key))
        }

        /// Returns all roles.
        pub fn roles(&self) -> impl Iterator<Item = crate::Result<&str>> + '_ {
            self.roles.entries().map(|(_, value)| value.name())
        }
    }
}

#[cfg(all(test, feature = "model"))]
mod tests {
    use super::*;
    use crate::constants::MARKET_USD_UNIT;

    /// Helper function to create a Store with specific GT and factor settings for testing.
    fn create_test_store(
        max_rank: u64,
        order_fee_discount_factors: [u128; 16],
        order_fee_discount_for_referred_user: u128,
    ) -> Store {
        let mut store: Store = Zeroable::zeroed();
        store.gt.max_rank = max_rank;
        store.gt.order_fee_discount_factors = order_fee_discount_factors;
        store.factor.order_fee_discount_for_referred_user = order_fee_discount_for_referred_user;
        store
    }

    #[test]
    fn test_order_fee_discount_factor_rank_0_no_referral() {
        // rank = 0, is_referred = false -> should return 0
        let discount_factors = [0u128; 16];
        let store = create_test_store(9, discount_factors, 0);

        let factor = store.order_fee_discount_factor(0, false).unwrap();
        assert_eq!(factor, 0, "rank 0 without referral should have 0 discount");
    }

    #[test]
    fn test_order_fee_discount_factor_rank_1_no_referral() {
        // rank = 1, is_referred = false -> should return 2.5%
        let mut discount_factors = [0u128; 16];
        discount_factors[1] = MARKET_USD_UNIT / 1_000 * 25; // 2.5%
        let store = create_test_store(9, discount_factors, 0);

        let factor = store.order_fee_discount_factor(1, false).unwrap();
        let expected = MARKET_USD_UNIT / 1_000 * 25; // 2.5%
        assert_eq!(
            factor, expected,
            "rank 1 without referral should have 2.5% discount"
        );
    }

    #[test]
    fn test_order_fee_discount_factor_rank_0_with_referral() {
        // rank = 0, is_referred = true -> should return 10% (referred discount only)
        let discount_factors = [0u128; 16];
        let referred_discount = MARKET_USD_UNIT / 100 * 10; // 10%
        let store = create_test_store(9, discount_factors, referred_discount);

        let factor = store.order_fee_discount_factor(0, true).unwrap();
        // Formula: 1 - (1 - 0) * (1 - 0.1) = 0.1 = 10%
        assert_eq!(
            factor, referred_discount,
            "rank 0 with referral should have 10% discount"
        );
    }

    #[test]
    fn test_order_fee_discount_factor_rank_1_with_referral() {
        // rank = 1, is_referred = true -> combined discount
        // Formula: 1 - (1 - A) * (1 - B) = A + B * (1 - A) = 0.025 + 0.10 * (1 - 0.025) = 0.025 + 0.10 * 0.975 = 0.025 + 0.0975 = 0.1225 = 12.25%
        // A = 2.5% (rank 1), B = 10% (referred)
        let mut discount_factors = [0u128; 16];
        let rank_discount = MARKET_USD_UNIT / 1_000 * 25; // 2.5%
        discount_factors[1] = rank_discount;
        let referred_discount = MARKET_USD_UNIT / 100 * 10; // 10%
        let store = create_test_store(9, discount_factors, referred_discount);

        let factor = store.order_fee_discount_factor(1, true).unwrap();

        let complement = MARKET_USD_UNIT - referred_discount; // 1 - 10% = 90%
        let expected = referred_discount + rank_discount * complement / MARKET_USD_UNIT;
        assert_eq!(
            factor, expected,
            "rank 1 with referral should have combined discount (12.25%)"
        );
    }

    #[test]
    fn test_order_fee_discount_factor_max_rank() {
        // Test with max_rank = 9, rank = 9
        let mut discount_factors = [0u128; 16];
        discount_factors[9] = MARKET_USD_UNIT / 1_000 * 225; // 22.5%
        let store = create_test_store(9, discount_factors, 0);

        let factor = store.order_fee_discount_factor(9, false).unwrap();
        let expected = MARKET_USD_UNIT / 1_000 * 225;
        assert_eq!(factor, expected, "max rank should have 22.5% discount");
    }

    #[test]
    fn test_order_fee_discount_factor_rank_exceeds_max() {
        // rank exceeds max_rank -> should return error
        let discount_factors = [0u128; 16];
        let store = create_test_store(9, discount_factors, 0);

        let result = store.order_fee_discount_factor(10, false);
        assert!(
            result.is_err(),
            "rank exceeding max_rank should return error"
        );
    }

    #[test]
    fn test_order_fee_discount_factor_full_setup() {
        // Test with full setup matching the integration test:
        // order_fee_discount_factors: [0, 25, 50, 75, 100, 125, 150, 175, 200, 225] * MARKET_USD_UNIT / 1_000
        // OrderFeeDiscountForReferredUser: 10% (10 * MARKET_USD_UNIT / 100)
        let discount_factors: [u128; 16] = [
            0,
            MARKET_USD_UNIT / 1_000 * 25,  // 2.5%
            MARKET_USD_UNIT / 1_000 * 50,  // 5%
            MARKET_USD_UNIT / 1_000 * 75,  // 7.5%
            MARKET_USD_UNIT / 1_000 * 100, // 10%
            MARKET_USD_UNIT / 1_000 * 125, // 12.5%
            MARKET_USD_UNIT / 1_000 * 150, // 15%
            MARKET_USD_UNIT / 1_000 * 175, // 17.5%
            MARKET_USD_UNIT / 1_000 * 200, // 20%
            MARKET_USD_UNIT / 1_000 * 225, // 22.5%
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let referred_discount = MARKET_USD_UNIT / 100 * 10; // 10%
        let store = create_test_store(9, discount_factors, referred_discount);

        // Test rank 0, no referral
        assert_eq!(store.order_fee_discount_factor(0, false).unwrap(), 0);

        // Test rank 5, no referral (12.5%)
        let expected_rank_5 = MARKET_USD_UNIT / 1_000 * 125;
        assert_eq!(
            store.order_fee_discount_factor(5, false).unwrap(),
            expected_rank_5
        );

        // Test rank 5, with referral
        // Formula: referred + rank_discount * (1 - referred) / UNIT = 10% + 12.5% * 90% = 10% + 11.25% = 21.25%
        use crate::constants::MARKET_DECIMALS;
        use gmsol_model::utils::apply_factor;
        let complement = MARKET_USD_UNIT - referred_discount;
        let expected_combined = referred_discount
            + apply_factor::<_, { MARKET_DECIMALS }>(&expected_rank_5, &complement).unwrap();
        assert_eq!(
            store.order_fee_discount_factor(5, true).unwrap(),
            expected_combined
        );
    }

    #[test]
    fn test_order_fee_discount_factor_high_discounts() {
        let mut discount_factors = [0u128; 16];
        discount_factors[0] = MARKET_USD_UNIT / 100 * 50; // 50%
        let referred_discount = MARKET_USD_UNIT / 100 * 40; // 40%
        let store = create_test_store(9, discount_factors, referred_discount);

        // Combined: 1 - (1 - 0.5) * (1 - 0.4) = 1 - 0.5 * 0.6 = 1 - 0.3 = 0.7 = 70%
        let factor = store.order_fee_discount_factor(0, true).unwrap();
        let expected = MARKET_USD_UNIT / 100 * 70;
        assert_eq!(factor, expected, "combined high discounts should be 70%");
    }
}
