use anchor_lang::prelude::*;

use gmsol_utils::price::Price;

use crate::{states::TokenConfig, CoreError};

/// The Chainlink Program.
pub struct Chainlink;

anchor_lang::solana_program::declare_id!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");

impl Id for Chainlink {
    fn id() -> Pubkey {
        ID
    }
}

impl Chainlink {
    /// Check and get latest chainlink price from data feed.
    pub(crate) fn check_and_get_chainlink_price<'info>(
        _clock: &Clock,
        _chainlink_program: &impl ToAccountInfo<'info>,
        _token_config: &TokenConfig,
        _feed: &AccountInfo<'info>,
    ) -> Result<(u64, i64, Price)> {
        // let chainlink_program = chainlink_program.to_account_info();
        // let round = chainlink_solana::latest_round_data(chainlink_program.clone(), feed.clone())
        //     .map_err(|err| error!(CoreError::InvalidPriceFeedPrice))?;
        // let decimals = chainlink_solana::decimals(chainlink_program, feed.clone())
        //     .map_err(|err| error!(CoreError::InvalidPriceFeedPrice))?;
        // Self::check_and_get_price_from_round(clock, &round, decimals, token_config)
        err!(CoreError::Unimplemented)
    }

    // /// Check and get price from the round data.
    // fn check_and_get_price_from_round(
    //     clock: &Clock,
    //     round: &chainlink_solana::Round,
    //     decimals: u8,
    //     token_config: &TokenConfig,
    // ) -> Result<(u64, i64, Price)> {
    //     let chainlink_solana::Round {
    //         answer, timestamp, ..
    //     } = round;
    //     require_gt!(*answer, 0, CoreError::InvalidPriceFeedPrice);
    //     let timestamp = *timestamp as i64;
    //     let current = clock.unix_timestamp;
    //     if current > timestamp && current - timestamp > token_config.heartbeat_duration().into() {
    //         return err!(CoreError::PriceFeedNotUpdated);
    //     }
    //     let price = Decimal::try_from_price(
    //         *answer as u128,
    //         decimals,
    //         token_config.token_decimals(),
    //         token_config.precision(),
    //     )
    //     .map_err(|_| error!(CoreError::InvalidPriceFeedPrice))?;
    //     Ok((
    //         round.slot,
    //         timestamp,
    //         Price {
    //             min: price,
    //             max: price,
    //         },
    //     ))
    // }
}
