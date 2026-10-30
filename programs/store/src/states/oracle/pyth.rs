use anchor_lang::prelude::*;
use gmsol_utils::oracle::OracleError;
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{states::TokenConfig, CoreError};

pub use gmsol_utils::oracle::pyth_price_with_confidence_to_price;

use super::OraclePriceParts;

/// The Pyth receiver program.
pub struct Pyth;

impl Id for Pyth {
    fn id() -> Pubkey {
        pyth_solana_receiver_sdk::ID
    }
}

impl Pyth {
    /// Push Oracle ID.
    pub const PUSH_ORACLE_ID: Pubkey = pyth_solana_receiver_sdk::PYTH_PUSH_ORACLE_ID;

    #[allow(clippy::manual_inspect)]
    pub(super) fn check_and_get_price<'info>(
        clock: &Clock,
        token_config: &TokenConfig,
        feed: &'info AccountInfo<'info>,
        feed_id: &Pubkey,
    ) -> Result<OraclePriceParts> {
        let feed = Account::<PriceUpdateV2>::try_from(feed)?;
        let feed_id = feed_id.to_bytes();
        let max_age = token_config.heartbeat_duration().into();
        let price = feed
            .get_price_no_older_than(clock, max_age, &feed_id)
            .map_err(|err| {
                let price_ts = feed.price_message.publish_time;
                msg!(
                    "[Pyth] get price error, clock={} price_ts={} max_age={}",
                    clock.unix_timestamp,
                    price_ts,
                    max_age,
                );
                err
            })?;
        let parsed_price = pyth_price_with_confidence_to_price(
            price.price,
            price.conf,
            price.exponent,
            token_config,
        )
        .map_err(CoreError::from)
        .map_err(|err| error!(err))?;
        Ok(OraclePriceParts {
            oracle_slot: feed.posted_slot,
            oracle_ts: price.publish_time,
            price: parsed_price,
            ref_price: None,
            is_open: true,
        })
    }
}

impl From<OracleError> for CoreError {
    fn from(err: OracleError) -> Self {
        msg!("Oracle error: {}", err);
        match err {
            OracleError::InvalidPriceFeedPrice(_) => CoreError::InvalidPriceFeedPrice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;
    use gmsol_utils::token_config::UpdateTokenConfigParams;
    use pyth_solana_receiver_sdk::{
        error::GetPriceError,
        price_update::{PriceFeedMessage, VerificationLevel},
    };

    #[test]
    fn default_heartbeat_accepts_pyth_prices_through_thirty_seconds() {
        let feed_id = Pubkey::new_unique();
        let update = PriceUpdateV2 {
            write_authority: Pubkey::new_unique(),
            verification_level: VerificationLevel::Full,
            price_message: PriceFeedMessage {
                feed_id: feed_id.to_bytes(),
                price: 1_000,
                conf: 1,
                exponent: 0,
                publish_time: 100,
                prev_publish_time: 99,
                ema_price: 1_000,
                ema_conf: 1,
            },
            posted_slot: 1,
        };
        let mut data = Vec::new();
        update
            .try_serialize(&mut data)
            .expect("Pyth account serializes");
        let mut lamports = 1;
        let key = Pubkey::new_unique();
        let owner = Pyth::id();
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );
        let mut config = TokenConfig::zeroed();
        config.heartbeat_duration = UpdateTokenConfigParams::default().heartbeat_duration;
        config.precision = 4;
        assert_eq!(config.heartbeat_duration, 30);

        for age in [29, 30] {
            let clock = Clock {
                unix_timestamp: 100 + age,
                ..Clock::default()
            };
            let parts = Pyth::check_and_get_price(&clock, &config, &account, &feed_id)
                .expect("a price within the default heartbeat is accepted");
            assert_eq!(parts.oracle_ts, 100);
        }

        let stale_clock = Clock {
            unix_timestamp: 131,
            ..Clock::default()
        };
        let err = Pyth::check_and_get_price(&stale_clock, &config, &account, &feed_id)
            .err()
            .expect("a price beyond the default heartbeat is rejected");
        match err {
            Error::AnchorError(err) => {
                assert_eq!(err.error_code_number, u32::from(GetPriceError::PriceTooOld));
            }
            other => panic!("unexpected stale Pyth price error: {other:?}"),
        }
    }
}
