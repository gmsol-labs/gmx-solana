use std::num::NonZeroU64;

use bytemuck::Zeroable;

use crate::gmsol_store::{
    accounts::{Glv, GtExchange, Market, Position, ReferralCodeV2, Store},
    types::{ActionHeader, EventPositionState, PositionState},
};

/// Referral Code Bytes.
pub type ReferralCodeBytes = [u8; 8];

impl Default for Market {
    fn default() -> Self {
        Zeroable::zeroed()
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

#[cfg(feature = "gmsol-utils")]
mod utils {
    use anchor_lang::prelude::Pubkey;
    use gmsol_utils::{
        action::{ActionCallbackKind, ActionFlag, MAX_ACTION_FLAGS},
        impl_fixed_map, impl_flags,
        market::{self, HasMarketMeta},
        order::{self, PositionKind, TradeFlag, TradeFlagContainer},
        pubkey::{self, optional_address},
        swap::{self, HasSwapParams},
        token_config::{self, TokensCollector},
    };

    use crate::gmsol_store::{
        accounts::{Glv, Position},
        events::TradeEvent,
        types::{
            ActionFlagContainer, ActionHeader, GlvMarketConfig, GlvMarkets, GlvMarketsEntry,
            MarketMeta, OrderActionParams, OrderKind, SwapActionParams, TokenAndAccount, Tokens,
            TokensEntry, UpdateTokenConfigParams,
        },
    };

    const MAX_TOKENS: usize = 256;
    const MAX_ALLOWED_NUMBER_OF_MARKETS: usize = 96;

    impl_fixed_map!(Tokens, Pubkey, pubkey::to_bytes, u8, MAX_TOKENS);
    impl_fixed_map!(
        GlvMarkets,
        Pubkey,
        pubkey::to_bytes,
        GlvMarketConfig,
        MAX_ALLOWED_NUMBER_OF_MARKETS
    );

    impl_flags!(ActionFlag, MAX_ACTION_FLAGS, u8);

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
            TokensCollector::new(action, self.num_markets())
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
}
