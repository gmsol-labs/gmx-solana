use anchor_lang::prelude::*;
use gmsol_callback::interface::ActionKind;

pub mod error;
pub mod instructions;
pub mod states;

pub use error::CompetitionError;
pub use instructions::*;

declare_id!("2AxuNr6euZPKQbTwNsLBjzFTZFAevA85F4PW9m9Dv8pc");

#[program]
pub mod gmsol_competition {
    use super::*;

    /// Initialize the global [`Competition`](crate::states::Competition) PDA.
    pub fn initialize_competition(
        ctx: Context<InitializeCompetition>,
        start_time: i64,
        end_time: i64,
        store_program: Pubkey,
    ) -> Result<()> {
        InitializeCompetition::invoke(ctx, start_time, end_time, store_program)
    }

    /// Create [`Participant`](crate::states::Participant) PDA idempotently.
    pub fn create_participant_idempotent(ctx: Context<CreateParticipantIdempotent>) -> Result<()> {
        CreateParticipantIdempotent::invoke(ctx)
    }

    // ---------------------------------------------------------------------
    // Callbacks expected by the GMX‑Solana store‑program
    // ---------------------------------------------------------------------

    /// Triggered immediately **after an order is created**.  
    /// The competition logic is unaffected, so this is a no‑op kept only
    /// for interface compatibility.
    pub fn on_created(
        _ctx: Context<OnCallback>,
        _authority_bump: u8,
        action_kind: u8,
        _extra_account_count: u8,
    ) -> Result<()> {
        // Only setup callback for orders.
        require!(
            action_kind == ActionKind::Order as u8,
            CompetitionError::InvalidActionKind
        );
        Ok(())
    }

    /// Triggered when an order is updated.  
    /// Currently ignored by the competition contract.
    pub fn on_updated(
        _ctx: Context<OnCallback>,
        _authority_bump: u8,
        _extra_account_count: u8,
    ) -> Result<()> {
        Ok(())
    }

    /// Triggered when an order is **executed**.  
    /// Updates the participant statistics and the on‑chain leaderboard.
    pub fn on_executed(
        ctx: Context<OnExecuted>,
        authority_bump: u8,
        action_kind: u8,
        success: bool,
        extra_account_count: u8,
    ) -> Result<()> {
        OnExecuted::invoke(
            ctx,
            authority_bump,
            action_kind,
            success,
            extra_account_count,
        )
    }

    /// Triggered when an order is **closed / cancelled**.  
    /// Currently ignored by the competition contract.
    pub fn on_closed(
        _ctx: Context<OnCallback>,
        _authority_bump: u8,
        _extra_account_count: u8,
    ) -> Result<()> {
        Ok(())
    }
}
