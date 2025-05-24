use anchor_lang::prelude::*;
use gmsol_utils::{oracle::OracleError, price::Price};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{states::TokenConfig, CoreError};

pub use gmsol_utils::oracle::pyth_price_with_confidence_to_price;

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
    ) -> Result<(u64, i64, Price)> {
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
        Ok((feed.posted_slot, price.publish_time, parsed_price))
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
