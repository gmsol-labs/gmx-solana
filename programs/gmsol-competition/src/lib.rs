use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod state;

pub use instructions::*;

declare_id!("2AxuNr6euZPKQbTwNsLBjzFTZFAevA85F4PW9m9Dv8pc");

#[program]
pub mod gmsol_competition {
    use super::*;

    pub fn initialize_competition(
        ctx: Context<InitializeCompetition>,
        start_time: i64,
        end_time: i64,
        store_program: Pubkey,
    ) -> Result<()> {
        instructions::init_competition_handler(ctx, start_time, end_time, store_program)
    }

    pub fn record_trade(ctx: Context<RecordTrade>, volume: u64) -> Result<()> {
        instructions::record_trade_handler(ctx, volume)
    }
}
